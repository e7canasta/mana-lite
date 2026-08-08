use crate::config::OccupancyPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomCardinality {
    Unknown,
    Empty,
    Single,
    Multiple,
}

impl RoomCardinality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
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
    pub empty_ticks: u32,
    pub multiple_candidate_ticks: u32,
    pub multiple_exit_ticks: u32,
}

pub struct OccupancyStateMachine {
    policy: OccupancyPolicy,
    state: RoomCardinality,
    second_person: SecondPersonState,
    empty_ticks: u32,
    multiple_candidate_ticks: u32,
    multiple_exit_ticks: u32,
}

impl OccupancyStateMachine {
    pub fn new(policy: OccupancyPolicy) -> Self {
        Self {
            policy,
            state: RoomCardinality::Unknown,
            second_person: SecondPersonState::None,
            empty_ticks: 0,
            multiple_candidate_ticks: 0,
            multiple_exit_ticks: 0,
        }
    }

    pub fn update(&mut self, evidence: OccupancyEvidence) -> OccupancyUpdate {
        if evidence.signal_valid {
            if evidence.raw_person_count >= 2 {
                self.empty_ticks = 0;
                self.multiple_exit_ticks = 0;
                self.multiple_candidate_ticks = self.multiple_candidate_ticks.saturating_add(1);
                self.second_person = if evidence.confirmed_person_count >= 2 {
                    SecondPersonState::Confirmed
                } else {
                    SecondPersonState::Candidate
                };

                if evidence.confirmed_person_count >= 2
                    && self.multiple_candidate_ticks >= self.policy.multiple_candidate_ticks
                {
                    self.state = RoomCardinality::Multiple;
                } else if self.state != RoomCardinality::Multiple && evidence.poi_present {
                    self.state = RoomCardinality::Single;
                }
            } else {
                self.multiple_candidate_ticks = 0;
                self.second_person = SecondPersonState::None;

                if self.state == RoomCardinality::Multiple {
                    self.multiple_exit_ticks = self.multiple_exit_ticks.saturating_add(1);
                    if self.multiple_exit_ticks >= self.policy.multiple_exit_ticks {
                        self.state = if evidence.poi_present {
                            RoomCardinality::Single
                        } else if evidence.raw_person_count > 0 {
                            RoomCardinality::Unknown
                        } else {
                            RoomCardinality::Empty
                        };
                        self.multiple_exit_ticks = 0;
                    }
                } else if evidence.poi_present {
                    self.state = RoomCardinality::Single;
                    self.empty_ticks = 0;
                    self.multiple_exit_ticks = 0;
                } else if evidence.raw_person_count > 0 {
                    // A candidate person is not evidence that the room is empty.
                    self.empty_ticks = 0;
                    self.multiple_exit_ticks = 0;
                } else {
                    self.empty_ticks = self.empty_ticks.saturating_add(1);
                    self.multiple_exit_ticks = 0;
                    if self.empty_ticks >= self.policy.empty_ticks {
                        self.state = RoomCardinality::Empty;
                    }
                }
            }
        }

        OccupancyUpdate {
            state: self.state,
            second_person: self.second_person,
            empty_ticks: self.empty_ticks,
            multiple_candidate_ticks: self.multiple_candidate_ticks,
            multiple_exit_ticks: self.multiple_exit_ticks,
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

    fn policy() -> OccupancyPolicy {
        OccupancyPolicy {
            empty_ticks: 3,
            multiple_candidate_ticks: 2,
            multiple_exit_ticks: 2,
        }
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

        assert_eq!(
            machine.update(evidence(1, true, 1)).state,
            RoomCardinality::Single
        );
        let update = machine.update(evidence(2, true, 1));

        assert_eq!(update.state, RoomCardinality::Single);
        assert_eq!(update.second_person, SecondPersonState::Candidate);
    }

    #[test]
    fn multiple_requires_two_valid_ticks_and_two_confirmed_tracks() {
        let mut machine = OccupancyStateMachine::new(policy());

        machine.update(evidence(1, true, 1));
        machine.update(evidence(2, true, 1));
        let update = machine.update(evidence(2, true, 2));
        assert_eq!(update.state, RoomCardinality::Multiple);
        assert_eq!(update.second_person, SecondPersonState::Confirmed);
    }

    #[test]
    fn multiple_returns_to_single_after_configured_exit_ticks() {
        let mut machine = OccupancyStateMachine::new(policy());

        machine.update(evidence(2, true, 2));
        machine.update(evidence(2, true, 2));
        assert_eq!(machine.state(), RoomCardinality::Multiple);

        machine.update(evidence(1, true, 1));
        let update = machine.update(evidence(1, true, 1));
        assert_eq!(update.state, RoomCardinality::Single);
    }

    #[test]
    fn invalid_signal_does_not_advance_the_state_machine() {
        let mut machine = OccupancyStateMachine::new(policy());

        machine.update(evidence(1, true, 1));
        let update = machine.update(OccupancyEvidence {
            signal_valid: false,
            raw_person_count: 0,
            poi_present: false,
            confirmed_person_count: 0,
        });

        assert_eq!(update.state, RoomCardinality::Single);
        assert_eq!(update.empty_ticks, 0);
    }

    #[test]
    fn empty_requires_configured_valid_ticks_without_poi() {
        let mut machine = OccupancyStateMachine::new(policy());

        for _ in 0..2 {
            assert_eq!(
                machine.update(evidence(0, false, 0)).state,
                RoomCardinality::Unknown
            );
        }
        assert_eq!(
            machine.update(evidence(0, false, 0)).state,
            RoomCardinality::Empty
        );
    }

    #[test]
    fn person_candidate_does_not_count_as_empty() {
        let mut machine = OccupancyStateMachine::new(policy());

        for _ in 0..5 {
            assert_eq!(
                machine.update(evidence(1, false, 0)).state,
                RoomCardinality::Unknown
            );
        }
    }
}
