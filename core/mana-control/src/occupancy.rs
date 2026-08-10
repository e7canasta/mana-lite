use crate::config::OccupancyPolicy;
use crate::timing::Dwell;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomCardinality {
    Empty,
    Single,
    Multiple,
}

/// Validity of the primary detection signal published alongside occupancy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalValidity {
    Valid,
    Invalid,
}

impl SignalValidity {
    #[must_use]
    pub const fn from_bool(valid: bool) -> Self {
        if valid { Self::Valid } else { Self::Invalid }
    }

    #[must_use]
    pub const fn is_valid(self) -> bool {
        matches!(self, Self::Valid)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
        }
    }
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

/// Inputs used to derive [`OccupancyEvidence`] from presence + tracking.
#[derive(Debug, Clone, Copy)]
pub struct OccupancyEvidenceInputs {
    pub tracking_enabled: bool,
    pub presence_enabled: bool,
    pub signal_valid: bool,
    pub raw_person_count: usize,
    pub confirmed_person_count: usize,
    pub presence_is_present: bool,
    pub presence_held: bool,
}

/// Pure clinical policy: how presence hold + tracking combine into occupancy
/// evidence. Extracted from the orchestrator so it can be table-tested.
#[must_use]
pub fn build_evidence(inputs: OccupancyEvidenceInputs) -> OccupancyEvidence {
    let poi_present = if inputs.presence_enabled {
        if inputs.tracking_enabled {
            inputs.presence_is_present
        } else {
            // Raw calibration uses the POI entry timer, but a missing raw
            // person starts the room exit timer immediately instead of
            // being held by presence.poi.off_ms.
            inputs.raw_person_count == 1 && inputs.presence_is_present
        }
    } else {
        inputs.raw_person_count == 1
    };

    let occupancy_person_count = if inputs.tracking_enabled
        && inputs.raw_person_count == 0
        && inputs.presence_held
        && inputs.confirmed_person_count == 1
        && poi_present
    {
        // Keep a confirmed single-person session alive across the short
        // detector dropouts already retained by PresenceFilter.
        1
    } else {
        inputs.raw_person_count
    };

    OccupancyEvidence {
        signal_valid: inputs.signal_valid,
        raw_person_count: occupancy_person_count,
        poi_present,
        confirmed_person_count: inputs.confirmed_person_count,
    }
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
    single: Dwell,
    empty: Dwell,
    multiple_candidate: Dwell,
    multiple_exit: Dwell,
}

impl OccupancyStateMachine {
    pub fn new(policy: OccupancyPolicy) -> Self {
        Self {
            policy,
            // Before the first valid frame, room cardinality is conservatively
            // empty; signal validity is published on its own lane.
            state: RoomCardinality::Empty,
            second_person: SecondPersonState::None,
            single: Dwell::new(),
            empty: Dwell::new(),
            multiple_candidate: Dwell::new(),
            multiple_exit: Dwell::new(),
        }
    }

    pub fn update_at(&mut self, evidence: OccupancyEvidence, now: Instant) -> OccupancyUpdate {
        if !evidence.signal_valid {
            // A TON/TOF requires a continuous valid condition. An invalid
            // inference freezes room state but cannot satisfy a timer.
            self.single.clear();
            self.empty.clear();
            self.multiple_candidate.clear();
            self.multiple_exit.clear();
            return self.snapshot(now);
        }

        if evidence.raw_person_count >= 2 {
            self.single.clear();
            self.empty.clear();
            self.multiple_exit.clear();
            let second_person_ready =
                !self.policy.require_confirmed_tracks || evidence.confirmed_person_count >= 2;
            if second_person_ready {
                self.multiple_candidate.start_or_keep(now);
                if self
                    .multiple_candidate
                    .ready(now, self.policy.multiple_confirm_ms)
                {
                    self.state = RoomCardinality::Multiple;
                    self.second_person = SecondPersonState::Confirmed;
                } else {
                    self.second_person = SecondPersonState::Candidate;
                }
            } else {
                self.multiple_candidate.clear();
                self.second_person = SecondPersonState::Candidate;
            }
        } else {
            self.multiple_candidate.clear();
            self.second_person = SecondPersonState::None;

            if self.state == RoomCardinality::Multiple {
                self.single.clear();
                self.empty.clear();
                self.multiple_exit.start_or_keep(now);
                if self.multiple_exit.ready(now, self.policy.multiple_exit_ms) {
                    self.state = if evidence.raw_person_count > 0 || evidence.poi_present {
                        RoomCardinality::Single
                    } else {
                        RoomCardinality::Empty
                    };
                    self.multiple_exit.clear();
                }
            } else if evidence.raw_person_count == 0
                && evidence.poi_present
                && self.state == RoomCardinality::Single
            {
                // Tracked profile may hold a recently confirmed POI through a
                // short dropout. Raw calibration passes false here.
                self.single.clear();
                self.empty.clear();
                self.multiple_exit.clear();
            } else if evidence.raw_person_count == 1 && evidence.poi_present {
                self.empty.clear();
                self.multiple_exit.clear();
                self.single.start_or_keep(now);
                if self.single.ready(now, self.policy.single_confirm_ms) {
                    self.state = RoomCardinality::Single;
                }
            } else if evidence.raw_person_count > 0 {
                // A raw candidate blocks the empty timer but cannot start
                // the room entry timer until POI presence is confirmed.
                self.single.clear();
                self.empty.clear();
                self.multiple_exit.clear();
            } else {
                self.single.clear();
                self.multiple_exit.clear();
                if self.state != RoomCardinality::Empty {
                    self.empty.start_or_keep(now);
                    if self.empty.ready(now, self.policy.empty_confirm_ms) {
                        self.state = RoomCardinality::Empty;
                        self.empty.clear();
                    }
                } else {
                    self.empty.clear();
                }
            }
        }

        self.snapshot(now)
    }

    fn snapshot(&self, now: Instant) -> OccupancyUpdate {
        OccupancyUpdate {
            state: self.state,
            second_person: self.second_person,
            single_timer_ms: self.single.elapsed_ms(now),
            empty_timer_ms: self.empty.elapsed_ms(now),
            multiple_candidate_timer_ms: self.multiple_candidate.elapsed_ms(now),
            multiple_exit_timer_ms: self.multiple_exit.elapsed_ms(now),
        }
    }

    #[cfg(test)]
    fn state(&self) -> RoomCardinality {
        self.state
    }
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
        let start = Instant::now(); // cfg(test)

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
        let start = Instant::now(); // cfg(test)

        machine.update_at(evidence(2, true, 1), at(start, 0));
        machine.update_at(evidence(2, true, 2), at(start, 0));
        let update = machine.update_at(evidence(2, true, 2), at(start, 2_000));
        assert_eq!(update.state, RoomCardinality::Multiple);
        assert_eq!(update.second_person, SecondPersonState::Confirmed);
    }

    #[test]
    fn multiple_returns_to_single_after_configured_exit_timer() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now(); // cfg(test)

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
        let start = Instant::now(); // cfg(test)

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
        let start = Instant::now(); // cfg(test)

        machine.update_at(evidence(1, true, 1), at(start, 0));
        machine.update_at(evidence(1, true, 1), at(start, 1_000));
        let update = machine.update_at(evidence(0, true, 0), at(start, 20_000));

        assert_eq!(update.state, RoomCardinality::Single);
        assert_eq!(update.empty_timer_ms, 0);
    }

    #[test]
    fn empty_requires_configured_valid_time_without_poi() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now(); // cfg(test)

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
        let start = Instant::now(); // cfg(test)

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
        let start = Instant::now(); // cfg(test)

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
        let start = Instant::now(); // cfg(test)

        machine.update_at(evidence(2, true, 0), at(start, 0));
        let update = machine.update_at(evidence(2, true, 0), at(start, 2_000));
        assert_eq!(update.state, RoomCardinality::Multiple);
    }

    #[test]
    fn irregular_keyframe_gaps_use_elapsed_time_not_tick_count() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now(); // cfg(test)

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

    #[test]
    fn build_evidence_holds_single_across_detector_dropout() {
        let evidence = build_evidence(OccupancyEvidenceInputs {
            tracking_enabled: true,
            presence_enabled: true,
            signal_valid: true,
            raw_person_count: 0,
            confirmed_person_count: 1,
            presence_is_present: true,
            presence_held: true,
        });
        assert_eq!(evidence.raw_person_count, 1);
        assert!(evidence.poi_present);
    }

    #[test]
    fn build_evidence_raw_mode_requires_exact_one_person_for_poi() {
        let evidence = build_evidence(OccupancyEvidenceInputs {
            tracking_enabled: false,
            presence_enabled: true,
            signal_valid: true,
            raw_person_count: 0,
            confirmed_person_count: 0,
            presence_is_present: true,
            presence_held: false,
        });
        assert!(!evidence.poi_present);
        assert_eq!(evidence.raw_person_count, 0);
    }

    #[test]
    fn build_evidence_without_presence_uses_raw_count() {
        let evidence = build_evidence(OccupancyEvidenceInputs {
            tracking_enabled: false,
            presence_enabled: false,
            signal_valid: true,
            raw_person_count: 1,
            confirmed_person_count: 0,
            presence_is_present: false,
            presence_held: false,
        });
        assert!(evidence.poi_present);
        assert_eq!(evidence.raw_person_count, 1);
    }

    #[test]
    fn multiple_to_empty_after_exit_then_empty_timer() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now(); // cfg(test)

        machine.update_at(evidence(2, true, 2), at(start, 0));
        machine.update_at(evidence(2, true, 2), at(start, 2_000));
        assert_eq!(machine.state(), RoomCardinality::Multiple);

        machine.update_at(evidence(0, false, 0), at(start, 2_000));
        let update = machine.update_at(evidence(0, false, 0), at(start, 4_000));
        assert_eq!(update.state, RoomCardinality::Empty);
        assert_eq!(update.second_person, SecondPersonState::None);
    }

    #[test]
    fn empty_to_multiple_direct_with_raw_mode() {
        let mut policy = policy();
        policy.require_confirmed_tracks = false;
        let mut machine = OccupancyStateMachine::new(policy);
        let start = Instant::now(); // cfg(test)

        assert_eq!(machine.state(), RoomCardinality::Empty);
        machine.update_at(evidence(2, true, 0), at(start, 0));
        let update = machine.update_at(evidence(2, true, 0), at(start, 2_000));
        assert_eq!(update.state, RoomCardinality::Multiple);
        assert_eq!(update.second_person, SecondPersonState::Confirmed);
    }

    #[test]
    fn second_person_confirmed_clears_to_none_when_back_to_single() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now(); // cfg(test)

        machine.update_at(evidence(2, true, 2), at(start, 0));
        machine.update_at(evidence(2, true, 2), at(start, 2_000));
        assert_eq!(machine.state(), RoomCardinality::Multiple);
        assert_eq!(
            machine
                .update_at(evidence(2, true, 2), at(start, 2_000))
                .second_person,
            SecondPersonState::Confirmed
        );

        machine.update_at(evidence(1, true, 1), at(start, 2_000));
        let update = machine.update_at(evidence(1, true, 1), at(start, 4_000));
        assert_eq!(update.state, RoomCardinality::Single);
        assert_eq!(update.second_person, SecondPersonState::None);
    }

    #[test]
    fn second_person_candidate_clears_when_raw_drops() {
        let mut machine = OccupancyStateMachine::new(policy());
        let start = Instant::now(); // cfg(test)

        machine.update_at(evidence(1, true, 1), at(start, 0));
        machine.update_at(evidence(1, true, 1), at(start, 1_000));
        let candidate = machine.update_at(evidence(2, true, 1), at(start, 1_000));
        assert_eq!(candidate.state, RoomCardinality::Single);
        assert_eq!(candidate.second_person, SecondPersonState::Candidate);

        let cleared = machine.update_at(evidence(1, true, 1), at(start, 1_500));
        assert_eq!(cleared.second_person, SecondPersonState::None);
    }
}
