use crate::config::OccupancyPolicy;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomCardinality {
    Empty,
    Single,
    Multiple,
}

impl RoomCardinality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Single => "single",
            Self::Multiple => "multiple",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondPersonState {
    None,
    Candidate,
    Confirmed,
}

impl SecondPersonState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Candidate => "candidate",
            Self::Confirmed => "confirmed",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OccupancyEvidence {
    pub signal_valid: bool,
    pub raw_person_count: usize,
    pub poi_present: bool,
    pub confirmed_person_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct OccupancyUpdate {
    pub state: RoomCardinality,
    pub second_person: SecondPersonState,
    pub single_timer_ms: u64,
    pub empty_timer_ms: u64,
    pub multiple_candidate_timer_ms: u64,
    pub multiple_exit_timer_ms: u64,
}

pub struct OccupancyStateMachine {
    policy: OccupancyPolicy,
    state: RoomCardinality,
    second_person: SecondPersonState,
    single_since: Option<Instant>,
    empty_since: Option<Instant>,
    multiple_candidate_since: Option<Instant>,
    multiple_exit_since: Option<Instant>,
}

impl OccupancyStateMachine {
    pub fn new(policy: OccupancyPolicy) -> Self {
        Self {
            policy,
            // Before the first valid frame, room cardinality is conservatively
            // empty; signal validity is published on its own lane.
            state: RoomCardinality::Empty,
            second_person: SecondPersonState::None,
            single_since: None,
            empty_since: None,
            multiple_candidate_since: None,
            multiple_exit_since: None,
        }
    }

    pub fn update_at(&mut self, evidence: OccupancyEvidence, now: Instant) -> OccupancyUpdate {
        if !evidence.signal_valid {
            // A TON/TOF requires a continuous valid condition. An invalid
            // inference freezes room state but cannot satisfy a timer.
            self.single_since = None;
            self.empty_since = None;
            self.multiple_candidate_since = None;
            self.multiple_exit_since = None;
            return self.snapshot(now);
        }

        if evidence.raw_person_count >= 2 {
            self.single_since = None;
            self.empty_since = None;
            self.multiple_exit_since = None;
            let second_person_ready =
                !self.policy.require_confirmed_tracks || evidence.confirmed_person_count >= 2;
            if second_person_ready {
                self.multiple_candidate_since.get_or_insert(now);
                if elapsed_ms(self.multiple_candidate_since, now) >= self.policy.multiple_confirm_ms
                {
                    self.state = RoomCardinality::Multiple;
                    self.second_person = SecondPersonState::Confirmed;
                } else {
                    self.second_person = SecondPersonState::Candidate;
                }
            } else {
                self.multiple_candidate_since = None;
                self.second_person = SecondPersonState::Candidate;
            }
        } else {
            self.multiple_candidate_since = None;
            self.second_person = SecondPersonState::None;

            if self.state == RoomCardinality::Multiple {
                self.single_since = None;
                self.empty_since = None;
                self.multiple_exit_since.get_or_insert(now);
                if elapsed_ms(self.multiple_exit_since, now) >= self.policy.multiple_exit_ms {
                    self.state = if evidence.raw_person_count > 0 || evidence.poi_present {
                        RoomCardinality::Single
                    } else {
                        RoomCardinality::Empty
                    };
                    self.multiple_exit_since = None;
                }
            } else if evidence.raw_person_count == 0
                && evidence.poi_present
                && self.state == RoomCardinality::Single
            {
                // The tracked profile may hold a recently confirmed POI
                // through a short dropout. Raw calibration passes false here.
                self.single_since = None;
                self.empty_since = None;
                self.multiple_exit_since = None;
            } else if evidence.raw_person_count == 1 && evidence.poi_present {
                self.empty_since = None;
                self.multiple_exit_since = None;
                self.single_since.get_or_insert(now);
                if elapsed_ms(self.single_since, now) >= self.policy.single_confirm_ms {
                    self.state = RoomCardinality::Single;
                }
            } else if evidence.raw_person_count > 0 {
                // A raw candidate blocks the empty timer but cannot start
                // the room entry timer until POI presence is confirmed.
                self.single_since = None;
                self.empty_since = None;
                self.multiple_exit_since = None;
            } else {
                self.single_since = None;
                self.multiple_exit_since = None;
                if self.state != RoomCardinality::Empty {
                    self.empty_since.get_or_insert(now);
                    if elapsed_ms(self.empty_since, now) >= self.policy.empty_confirm_ms {
                        self.state = RoomCardinality::Empty;
                        self.empty_since = None;
                    }
                }
            }
        }

        self.snapshot(now)
    }

    fn snapshot(&self, now: Instant) -> OccupancyUpdate {
        OccupancyUpdate {
            state: self.state,
            second_person: self.second_person,
            single_timer_ms: elapsed_ms(self.single_since, now),
            empty_timer_ms: elapsed_ms(self.empty_since, now),
            multiple_candidate_timer_ms: elapsed_ms(self.multiple_candidate_since, now),
            multiple_exit_timer_ms: elapsed_ms(self.multiple_exit_since, now),
        }
    }

    #[cfg(test)]
    fn state(&self) -> RoomCardinality {
        self.state
    }
}

fn elapsed_ms(since: Option<Instant>, now: Instant) -> u64 {
    since
        .map(|start| now.saturating_duration_since(start).as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn policy() -> OccupancyPolicy {
        OccupancyPolicy {
            single_confirm_ms: 1_000,
            empty_confirm_ms: 3_000,
            multiple_confirm_ms: 2_000,
            multiple_exit_ms: 2_000,
            require_confirmed_tracks: true,
        }
    }

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    fn evidence(raw: usize, poi: bool, confirmed: usize) -> OccupancyEvidence {
        OccupancyEvidence {
            signal_valid: true,
            raw_person_count: raw,
            poi_present: poi,
            confirmed_person_count: confirmed,
        }
    }

    #[test]
    fn one_person_is_not_replaced_by_a_single_second_person_candidate() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now();

        machine.update_at(evidence(1, true, 1), at(start, 0));
        assert_eq!(
            machine
                .update_at(evidence(1, true, 1), at(start, 1_000))
                .state,
            RoomCardinality::Single
        );
        let update = machine.update_at(evidence(2, true, 1), at(start, 1_000));

        assert_eq!(update.state, RoomCardinality::Single);
        assert_eq!(update.second_person, SecondPersonState::Candidate);
    }

    #[test]
    fn multiple_requires_sustained_time_and_two_confirmed_tracks() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now();

        machine.update_at(evidence(2, true, 1), at(start, 0));
        machine.update_at(evidence(2, true, 2), at(start, 0));
        let update = machine.update_at(evidence(2, true, 2), at(start, 2_000));
        assert_eq!(update.state, RoomCardinality::Multiple);
        assert_eq!(update.second_person, SecondPersonState::Confirmed);
    }

    #[test]
    fn multiple_returns_to_single_after_configured_exit_timer() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now();

        machine.update_at(evidence(2, true, 2), at(start, 0));
        machine.update_at(evidence(2, true, 2), at(start, 2_000));
        assert_eq!(machine.state(), RoomCardinality::Multiple);

        machine.update_at(evidence(1, true, 1), at(start, 2_000));
        let update = machine.update_at(evidence(1, true, 1), at(start, 4_000));
        assert_eq!(update.state, RoomCardinality::Single);
    }

    #[test]
    fn invalid_signal_does_not_advance_the_state_machine() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now();

        machine.update_at(evidence(1, true, 1), at(start, 0));
        machine.update_at(evidence(1, true, 1), at(start, 1_000));
        let update = machine.update_at(
            OccupancyEvidence {
                signal_valid: false,
                raw_person_count: 0,
                poi_present: false,
                confirmed_person_count: 0,
            },
            at(start, 10_000),
        );

        assert_eq!(update.state, RoomCardinality::Single);
        assert_eq!(update.empty_timer_ms, 0);
    }

    #[test]
    fn held_poi_does_not_start_room_exit_timer() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now();

        machine.update_at(evidence(1, true, 1), at(start, 0));
        machine.update_at(evidence(1, true, 1), at(start, 1_000));
        let update = machine.update_at(evidence(0, true, 0), at(start, 20_000));

        assert_eq!(update.state, RoomCardinality::Single);
        assert_eq!(update.empty_timer_ms, 0);
    }

    #[test]
    fn empty_requires_configured_valid_time_without_poi() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now();

        assert_eq!(
            machine.update_at(evidence(1, true, 1), at(start, 0)).state,
            RoomCardinality::Empty
        );
        assert_eq!(
            machine
                .update_at(evidence(1, true, 1), at(start, 1_000))
                .state,
            RoomCardinality::Single
        );
        assert_eq!(
            machine
                .update_at(evidence(0, false, 0), at(start, 1_000))
                .state,
            RoomCardinality::Single
        );
        assert_eq!(
            machine
                .update_at(evidence(0, false, 0), at(start, 3_999))
                .state,
            RoomCardinality::Single
        );
        assert_eq!(
            machine
                .update_at(evidence(0, false, 0), at(start, 4_000))
                .state,
            RoomCardinality::Empty
        );
    }

    #[test]
    fn room_starts_empty_before_first_valid_frame() {
        let machine = OccupancyStateMachine::new(policy());

        assert_eq!(machine.state(), RoomCardinality::Empty);
    }

    #[test]
    fn single_raw_person_releases_to_empty() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now();

        machine.update_at(evidence(1, true, 1), at(start, 0));
        machine.update_at(evidence(1, true, 1), at(start, 1_000));
        assert_eq!(
            machine
                .update_at(evidence(0, false, 0), at(start, 1_000))
                .state,
            RoomCardinality::Single
        );
        assert_eq!(
            machine
                .update_at(evidence(0, false, 0), at(start, 4_000))
                .state,
            RoomCardinality::Empty
        );
    }

    #[test]
    fn person_candidate_does_not_count_as_empty() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now();

        for _ in 0..5 {
            assert_eq!(
                machine.update_at(evidence(1, false, 0), at(start, 0)).state,
                RoomCardinality::Empty
            );
        }
    }

    #[test]
    fn raw_mode_confirms_multiple_without_tracking() {
        let mut policy = policy();
        policy.require_confirmed_tracks = false;
        let mut machine = OccupancyStateMachine::new(policy);
        let start = Instant::now();

        machine.update_at(evidence(2, true, 0), at(start, 0));
        let update = machine.update_at(evidence(2, true, 0), at(start, 2_000));
        assert_eq!(update.state, RoomCardinality::Multiple);
    }

    #[test]
    fn irregular_keyframe_gaps_use_elapsed_time_not_tick_count() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now();

        machine.update_at(evidence(1, true, 1), at(start, 0));
        machine.update_at(evidence(1, true, 1), at(start, 1_000));
        assert_eq!(
            machine
                .update_at(evidence(0, false, 0), at(start, 1_000))
                .state,
            RoomCardinality::Single
        );
        assert_eq!(
            machine
                .update_at(evidence(0, false, 0), at(start, 9_000))
                .state,
            RoomCardinality::Empty
        );
    }
}
