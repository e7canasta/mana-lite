use crate::config::PresencePoiPolicy;
use crate::detection::ConsolidatedObservation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceState {
    Absent,
    Present,
    Ambiguous,
}

impl PresenceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Present => "present",
            Self::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresenceUpdate {
    pub state: PresenceState,
    pub held: bool,
    pub positive_ms: u64,
    pub empty_ms: u64,
}

/// Debounces the primary presence signal without assigning identity.
pub struct PresenceFilter {
    enabled: bool,
    class: String,
    policy: PresencePoiPolicy,
    state: PresenceState,
    positive_ms: u64,
    empty_ms: u64,
    last_person: Option<ConsolidatedObservation>,
}

impl PresenceFilter {
    pub fn new(enabled: bool, class: impl Into<String>, policy: PresencePoiPolicy) -> Self {
        Self {
            enabled,
            class: class.into(),
            policy,
            state: PresenceState::Absent,
            positive_ms: 0,
            empty_ms: 0,
            last_person: None,
        }
    }

    /// Returns observations for the tracker. During a short valid dropout, the
    /// last consolidated person is held as a signal, not as a new identity.
    /// `dt_ms` is the real elapsed time between processed keyframes; invalid
    /// signal time (`signal_valid = false`) does not accumulate.
    pub fn update(
        &mut self,
        observations: &[ConsolidatedObservation],
        signal_valid: bool,
        dt_ms: u64,
    ) -> (Vec<ConsolidatedObservation>, PresenceUpdate) {
        if !self.enabled {
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    positive_ms: self.positive_ms,
                    empty_ms: self.empty_ms,
                },
            );
        }
        if !signal_valid {
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    positive_ms: self.positive_ms,
                    empty_ms: self.empty_ms,
                },
            );
        }

        let person_count = observations
            .iter()
            .filter(|observation| observation.class == self.class)
            .count();

        if person_count > 1 {
            self.state = PresenceState::Ambiguous;
            self.positive_ms = 0;
            self.empty_ms = 0;
            self.last_person = None;
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    positive_ms: self.positive_ms,
                    empty_ms: 0,
                },
            );
        }

        if let Some(person) = observations
            .iter()
            .find(|observation| observation.class == self.class)
        {
            self.positive_ms = self.positive_ms.saturating_add(dt_ms);
            self.empty_ms = 0;
            self.last_person = Some(person.clone());
            if self.positive_ms >= self.policy.on_ms {
                self.state = PresenceState::Present;
            }
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    positive_ms: self.positive_ms,
                    empty_ms: 0,
                },
            );
        }

        self.positive_ms = 0;
        self.empty_ms = self.empty_ms.saturating_add(dt_ms);
        if self.state == PresenceState::Present
            && self.last_person.is_some()
            && self.empty_ms < self.policy.off_ms
        {
            let mut held = observations.to_vec();
            held.push(self.last_person.as_ref().expect("checked above").clone());
            return (
                held,
                PresenceUpdate {
                    state: self.state,
                    held: true,
                    positive_ms: self.positive_ms,
                    empty_ms: self.empty_ms,
                },
            );
        }

        if self.empty_ms >= self.policy.off_ms {
            self.state = PresenceState::Absent;
            self.last_person = None;
        }

        (
            observations.to_vec(),
            PresenceUpdate {
                state: self.state,
                held: false,
                positive_ms: self.positive_ms,
                empty_ms: self.empty_ms,
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

    const DT_MS: u64 = 200;

    fn config(off_ms: u64) -> PresencePoiPolicy {
        PresencePoiPolicy {
            on_ms: 200,
            off_ms,
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
        let mut filter = PresenceFilter::new(true, "person", config(800));
        let one = [person()];
        let (observations, update) = filter.update(&one, true, DT_MS);
        assert_eq!(observations.len(), 1);
        assert_eq!(update.state, PresenceState::Present);

        let (observations, update) = filter.update(&[], true, DT_MS);
        assert_eq!(observations.len(), 1);
        assert!(update.held);
        assert_eq!(update.empty_ms, DT_MS);
        assert_eq!(filter.state(), PresenceState::Present);
    }

    #[test]
    fn confirms_presence_after_configured_on_ms() {
        let mut filter = PresenceFilter::new(
            true,
            "person",
            PresencePoiPolicy {
                on_ms: 600,
                off_ms: 600,
            },
        );
        let one = [person()];

        assert_eq!(filter.update(&one, true, DT_MS).1.state, PresenceState::Absent);
        assert_eq!(filter.update(&one, true, DT_MS).1.state, PresenceState::Absent);
        assert_eq!(filter.update(&one, true, DT_MS).1.state, PresenceState::Present);
    }

    #[test]
    fn releases_presence_after_configured_empty_ms() {
        let mut filter = PresenceFilter::new(true, "person", config(600));
        filter.update(&[person()], true, DT_MS);
        filter.update(&[], true, DT_MS);
        filter.update(&[], true, DT_MS);
        let (observations, update) = filter.update(&[], true, DT_MS);
        assert!(observations.is_empty());
        assert!(!update.held);
        assert_eq!(update.state, PresenceState::Absent);
    }

    #[test]
    fn invalid_signal_does_not_count_as_absence() {
        let mut filter = PresenceFilter::new(true, "person", config(400));
        filter.update(&[person()], true, DT_MS);
        let (observations, update) = filter.update(&[], false, DT_MS);
        assert!(observations.is_empty());
        assert!(!update.held);
        assert_eq!(update.empty_ms, 0);
        assert_eq!(update.state, PresenceState::Present);
    }

    #[test]
    fn multiple_people_are_ambiguous_and_never_held() {
        let mut filter = PresenceFilter::new(true, "person", config(800));
        let two = [person(), person()];
        let (observations, update) = filter.update(&two, true, DT_MS);
        assert_eq!(observations.len(), 2);
        assert_eq!(update.state, PresenceState::Ambiguous);
        assert!(!update.held);
    }

    #[test]
    fn hold_window_is_time_based_not_tick_based() {
        // off_ms = 800: la retencion dura 800 ms reales, no 4 frames.
        let mut filter = PresenceFilter::new(
            true,
            "person",
            PresencePoiPolicy {
                on_ms: 200,
                off_ms: 800,
            },
        );
        filter.update(&[person()], true, 200);

        // Un solo tick lento (camara degradada a 1 fps) ya agota la ventana.
        let (observations, update) = filter.update(&[], true, 1_000);
        assert!(!update.held, "1000 ms > off_ms: no debe retener");
        assert_eq!(update.state, PresenceState::Absent);
        assert!(observations.is_empty());
    }
}
