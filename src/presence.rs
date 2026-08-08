use crate::config::PresenceConfig;
use crate::detection::ConsolidatedObservation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceState {
    Absent,
    Present,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresenceUpdate {
    pub state: PresenceState,
    pub held: bool,
    pub empty_ticks: u32,
}

/// Debounces the primary presence signal without assigning identity.
pub struct PresenceFilter {
    config: PresenceConfig,
    state: PresenceState,
    positive_ticks: u32,
    empty_ticks: u32,
    last_person: Option<ConsolidatedObservation>,
}

impl PresenceFilter {
    pub fn new(config: PresenceConfig) -> Self {
        Self {
            config,
            state: PresenceState::Absent,
            positive_ticks: 0,
            empty_ticks: 0,
            last_person: None,
        }
    }

    /// Returns observations for the tracker. During a short valid dropout, the
    /// last consolidated person is held as a signal, not as a new identity.
    pub fn update(
        &mut self,
        observations: &[ConsolidatedObservation],
        signal_valid: bool,
    ) -> (Vec<ConsolidatedObservation>, PresenceUpdate) {
        if !self.config.enabled {
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    empty_ticks: self.empty_ticks,
                },
            );
        }
        if !signal_valid {
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    empty_ticks: self.empty_ticks,
                },
            );
        }

        let person_count = observations
            .iter()
            .filter(|observation| observation.class == self.config.class)
            .count();

        if person_count > 1 {
            self.state = PresenceState::Ambiguous;
            self.positive_ticks = 0;
            self.empty_ticks = 0;
            self.last_person = None;
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    empty_ticks: 0,
                },
            );
        }

        if let Some(person) = observations
            .iter()
            .find(|observation| observation.class == self.config.class)
        {
            self.positive_ticks = self.positive_ticks.saturating_add(1);
            self.empty_ticks = 0;
            self.last_person = Some(person.clone());
            if self.positive_ticks >= self.config.on_ticks {
                self.state = PresenceState::Present;
            }
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    empty_ticks: 0,
                },
            );
        }

        self.positive_ticks = 0;
        self.empty_ticks = self.empty_ticks.saturating_add(1);
        if self.state == PresenceState::Present
            && self.last_person.is_some()
            && self.empty_ticks < self.config.off_ticks
        {
            let mut held = observations.to_vec();
            held.push(self.last_person.as_ref().expect("checked above").clone());
            return (
                held,
                PresenceUpdate {
                    state: self.state,
                    held: true,
                    empty_ticks: self.empty_ticks,
                },
            );
        }

        if self.empty_ticks >= self.config.off_ticks {
            self.state = PresenceState::Absent;
            self.last_person = None;
        }

        (
            observations.to_vec(),
            PresenceUpdate {
                state: self.state,
                held: false,
                empty_ticks: self.empty_ticks,
            },
        )
    }

    #[cfg(test)]
    fn state(&self) -> PresenceState {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::DetectionEvidence;

    fn config(off_ticks: u32) -> PresenceConfig {
        PresenceConfig {
            enabled: true,
            class: "person".into(),
            on_ticks: 1,
            off_ticks,
        }
    }

    fn person() -> ConsolidatedObservation {
        ConsolidatedObservation {
            class: "person".into(),
            confidence: 0.9,
            bbox: [0.0, 0.0, 100.0, 100.0],
            primary_model: "detect-fast".into(),
            evidence: vec![DetectionEvidence {
                model: "detect-fast".into(),
                class: "person".into(),
                confidence: 0.9,
                bbox: [0.0, 0.0, 100.0, 100.0],
                mask: None,
            }],
            components: Vec::new(),
        }
    }

    #[test]
    fn holds_one_person_during_short_valid_dropout() {
        let mut filter = PresenceFilter::new(config(4));
        let one = [person()];
        let (observations, update) = filter.update(&one, true);
        assert_eq!(observations.len(), 1);
        assert_eq!(update.state, PresenceState::Present);

        let (observations, update) = filter.update(&[], true);
        assert_eq!(observations.len(), 1);
        assert!(update.held);
        assert_eq!(update.empty_ticks, 1);
        assert_eq!(filter.state(), PresenceState::Present);
    }

    #[test]
    fn releases_presence_after_configured_empty_ticks() {
        let mut filter = PresenceFilter::new(config(3));
        filter.update(&[person()], true);
        filter.update(&[], true);
        filter.update(&[], true);
        let (observations, update) = filter.update(&[], true);
        assert!(observations.is_empty());
        assert!(!update.held);
        assert_eq!(update.state, PresenceState::Absent);
    }

    #[test]
    fn invalid_signal_does_not_count_as_absence() {
        let mut filter = PresenceFilter::new(config(2));
        filter.update(&[person()], true);
        let (observations, update) = filter.update(&[], false);
        assert!(observations.is_empty());
        assert!(!update.held);
        assert_eq!(update.empty_ticks, 0);
        assert_eq!(update.state, PresenceState::Present);
    }

    #[test]
    fn multiple_people_are_ambiguous_and_never_held() {
        let mut filter = PresenceFilter::new(config(4));
        let two = [person(), person()];
        let (observations, update) = filter.update(&two, true);
        assert_eq!(observations.len(), 2);
        assert_eq!(update.state, PresenceState::Ambiguous);
        assert!(!update.held);
    }
}
