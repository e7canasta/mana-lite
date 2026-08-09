use crate::config::PresencePoiPolicy;
use crate::detection::ConsolidatedObservation;
use crate::timing::Debouncer;
use std::time::Instant;

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
    debouncer: Debouncer,
    last_person: Option<ConsolidatedObservation>,
}

impl PresenceFilter {
    pub fn new(enabled: bool, class: impl Into<String>, policy: PresencePoiPolicy) -> Self {
        Self {
            enabled,
            class: class.into(),
            policy,
            state: PresenceState::Absent,
            debouncer: Debouncer::new(),
            last_person: None,
        }
    }

    /// Returns observations for the tracker. During a short valid dropout, the
    /// last consolidated person is held as a signal, not as a new identity.
    /// Invalid signal time (`signal_valid = false`) does not accumulate.
    pub fn update_at(
        &mut self,
        observations: &[ConsolidatedObservation],
        signal_valid: bool,
        now: Instant,
    ) -> (Vec<ConsolidatedObservation>, PresenceUpdate) {
        if !self.enabled {
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    positive_ms: self.debouncer.elapsed_high_ms(now),
                    empty_ms: self.debouncer.elapsed_low_ms(now),
                },
            );
        }
        if !signal_valid {
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    positive_ms: self.debouncer.elapsed_high_ms(now),
                    empty_ms: self.debouncer.elapsed_low_ms(now),
                },
            );
        }

        let person_count = observations
            .iter()
            .filter(|observation| observation.class == self.class)
            .count();

        if person_count > 1 {
            self.state = PresenceState::Ambiguous;
            self.debouncer.reset();
            self.last_person = None;
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    positive_ms: 0,
                    empty_ms: 0,
                },
            );
        }

        if let Some(person) = observations
            .iter()
            .find(|observation| observation.class == self.class)
        {
            let engaged =
                self.debouncer
                    .update_at(true, self.policy.on_ms, self.policy.off_ms, now);
            self.last_person = Some(person.clone());
            if engaged {
                self.state = PresenceState::Present;
            }
            return (
                observations.to_vec(),
                PresenceUpdate {
                    state: self.state,
                    held: false,
                    positive_ms: self.debouncer.elapsed_high_ms(now),
                    empty_ms: 0,
                },
            );
        }

        let engaged =
            self.debouncer
                .update_at(false, self.policy.on_ms, self.policy.off_ms, now);
        let empty_ms = self.debouncer.elapsed_low_ms(now);
        if self.state == PresenceState::Present
            && self.last_person.is_some()
            && engaged
        {
            let mut held = observations.to_vec();
            held.push(self.last_person.as_ref().expect("checked above").clone());
            return (
                held,
                PresenceUpdate {
                    state: self.state,
                    held: true,
                    positive_ms: self.debouncer.elapsed_high_ms(now),
                    empty_ms,
                },
            );
        }

        if !engaged {
            self.state = PresenceState::Absent;
            self.last_person = None;
        }

        (
            observations.to_vec(),
            PresenceUpdate {
                state: self.state,
                held: false,
                positive_ms: self.debouncer.elapsed_high_ms(now),
                empty_ms,
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
    use std::time::Duration;

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
        let start = Instant::now();
        filter.update_at(&one, true, start);
        let (observations, update) =
            filter.update_at(&one, true, start + Duration::from_millis(DT_MS));
        assert_eq!(observations.len(), 1);
        assert_eq!(update.state, PresenceState::Present);

        let (observations, update) =
            filter.update_at(&[], true, start + Duration::from_millis(DT_MS * 2));
        assert_eq!(observations.len(), 1);
        assert!(update.held);
        assert_eq!(update.empty_ms, 0);
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
        let start = Instant::now();

        assert_eq!(filter.update_at(&one, true, start).1.state, PresenceState::Absent);
        assert_eq!(filter.update_at(&one, true, start + Duration::from_millis(DT_MS)).1.state, PresenceState::Absent);
        assert_eq!(filter.update_at(&one, true, start + Duration::from_millis(DT_MS * 3)).1.state, PresenceState::Present);
    }

    #[test]
    fn releases_presence_after_configured_empty_ms() {
        let mut filter = PresenceFilter::new(true, "person", config(600));
        let start = Instant::now();
        filter.update_at(&[person()], true, start);
        filter.update_at(&[person()], true, start + Duration::from_millis(DT_MS));
        filter.update_at(&[], true, start + Duration::from_millis(DT_MS * 2));
        filter.update_at(&[], true, start + Duration::from_millis(DT_MS * 3));
        let (observations, update) =
            filter.update_at(&[], true, start + Duration::from_millis(DT_MS * 5));
        assert!(observations.is_empty());
        assert!(!update.held);
        assert_eq!(update.state, PresenceState::Absent);
    }

    #[test]
    fn invalid_signal_does_not_count_as_absence() {
        let mut filter = PresenceFilter::new(true, "person", config(400));
        let start = Instant::now();
        filter.update_at(&[person()], true, start);
        filter.update_at(&[person()], true, start + Duration::from_millis(DT_MS));
        let (observations, update) =
            filter.update_at(&[], false, start + Duration::from_millis(DT_MS * 2));
        assert!(observations.is_empty());
        assert!(!update.held);
        assert_eq!(update.empty_ms, 0);
        assert_eq!(update.state, PresenceState::Present);
    }

    #[test]
    fn multiple_people_are_ambiguous_and_never_held() {
        let mut filter = PresenceFilter::new(true, "person", config(800));
        let two = [person(), person()];
        let (observations, update) = filter.update_at(&two, true, Instant::now());
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
        let start = Instant::now();
        filter.update_at(&[person()], true, start);
        filter.update_at(&[person()], true, start + Duration::from_millis(200));

        // Un solo tick lento (camara degradada a 1 fps) ya agota la ventana.
        filter.update_at(&[], true, start + Duration::from_millis(200));
        let (observations, update) =
            filter.update_at(&[], true, start + Duration::from_millis(1_200));
        assert!(!update.held, "1000 ms > off_ms: no debe retener");
        assert_eq!(update.state, PresenceState::Absent);
        assert!(observations.is_empty());
    }
}
