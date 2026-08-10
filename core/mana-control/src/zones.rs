use std::collections::HashMap;
use std::time::Instant;

use crate::config::ZoneCatalog;
use crate::timing::Dwell;
use crate::track::Track;

#[derive(Debug, Clone)]
pub enum ZoneEvent {
    Occupied {
        zone: String,
        label: Option<String>,
        #[allow(dead_code)]
        track_id: u64,
        class: String,
        confidence: f32,
    },
    Vacated {
        zone: String,
        label: Option<String>,
        #[allow(dead_code)]
        track_id: u64,
        class: String,
    },
}

struct ZoneState {
    rect: [f32; 4],
    label: Option<String>,
    hysteresis_ms: u64,
    is_occupied: bool,
    occupied_by: Vec<u64>,
    vacated: Dwell,
}

pub struct ZoneEngine {
    zones: HashMap<String, ZoneState>,
}

impl ZoneEngine {
    pub fn from_catalog(catalog: &ZoneCatalog) -> Self {
        let mut zones = HashMap::new();
        for (name, entry) in &catalog.zones {
            zones.insert(
                name.clone(),
                ZoneState {
                    rect: [
                        entry.x1 as f32,
                        entry.y1 as f32,
                        entry.x2 as f32,
                        entry.y2 as f32,
                    ],
                    label: entry.label.clone(),
                    hysteresis_ms: entry.hysteresis_ms,
                    is_occupied: false,
                    occupied_by: Vec::new(),
                    vacated: Dwell::new(),
                },
            );
        }
        Self { zones }
    }

    pub fn evaluate_at(&mut self, tracks: &[&Track], now: Instant) -> Vec<ZoneEvent> {
        let mut events = Vec::new();

        for (zone_name, state) in &mut self.zones {
            let label = state.label.clone();
            let current: Vec<u64> = tracks
                .iter()
                .filter(|t| t.is_confirmed && rect_intersects(&t.bbox, &state.rect))
                .map(|t| t.id)
                .collect();

            if !current.is_empty() {
                if !state.is_occupied {
                    state.is_occupied = true;
                    state.occupied_by = current.clone();
                    state.vacated.clear();
                    for &track_id in &current {
                        let (class, conf) = find_class_confidence(tracks, track_id);
                        events.push(ZoneEvent::Occupied {
                            zone: zone_name.clone(),
                            label: label.clone(),
                            track_id,
                            class,
                            confidence: conf,
                        });
                    }
                }
                continue;
            }

            if state.is_occupied {
                state.vacated.start_or_keep(now);
                if state.vacated.ready(now, state.hysteresis_ms) {
                    state.is_occupied = false;
                    let prev = std::mem::take(&mut state.occupied_by);
                    state.vacated.clear();
                    for track_id in prev {
                        let (class, _) = find_class_confidence(tracks, track_id);
                        events.push(ZoneEvent::Vacated {
                            zone: zone_name.clone(),
                            label: label.clone(),
                            track_id,
                            class,
                        });
                    }
                }
            }
        }

        events
    }

    pub fn all_vacant(&self) -> bool {
        self.zones.values().all(|z| !z.is_occupied)
    }

    pub fn is_occupied(&self, zone: &str) -> bool {
        self.zones.get(zone).is_some_and(|state| state.is_occupied)
    }
}

fn find_class_confidence(tracks: &[&Track], id: u64) -> (String, f32) {
    tracks
        .iter()
        .find(|t| t.id == id)
        .map(|t| (t.class.to_string(), t.confidence))
        .unwrap_or_else(|| ("unknown".into(), 0.0))
}

fn rect_intersects(bbox: &[f32; 4], zone: &[f32; 4]) -> bool {
    bbox[0] < zone[2] && bbox[2] > zone[0] && bbox[1] < zone[3] && bbox[3] > zone[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ZoneSpec;

    fn make_track(id: u64, class: &str, bbox: [f32; 4], confirmed: bool) -> Track {
        Track {
            id,
            source_model: "detect-fast".into(),
            class: class.into(),
            bbox,
            confidence: 0.9,
            evidence: Vec::new(),
            kalman: crate::kalman::Kalman7::default(),
            hits: if confirmed { 3 } else { 1 },
            hit_streak: if confirmed { 3 } else { 1 },
            misses: 0,
            age: 3,
            time_since_update_ms: 0,
            is_confirmed: confirmed,
        }
    }

    #[test]
    fn occupied_triggers_event() {
        let catalog = ZoneCatalog {
            zones: HashMap::from([(
                "bed".into(),
                ZoneSpec {
                    x1: 0,
                    y1: 0,
                    x2: 200,
                    y2: 200,
                    label: None,
                    hysteresis_ms: 100,
                },
            )]),
            face_dwell: None,
        };
        let mut engine = ZoneEngine::from_catalog(&catalog);
        let track = make_track(1, "person", [50.0, 50.0, 150.0, 150.0], true);
        let events = engine.evaluate_at(&[&track], Instant::now()); // cfg(test)
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ZoneEvent::Occupied { zone, .. } if zone == "bed"))
        );
    }

    #[test]
    fn vacated_after_hysteresis() {
        let catalog = ZoneCatalog {
            zones: HashMap::from([(
                "bed".into(),
                ZoneSpec {
                    x1: 0,
                    y1: 0,
                    x2: 500,
                    y2: 500,
                    label: None,
                    hysteresis_ms: 0, // zero hysteresis for test
                },
            )]),
            face_dwell: None,
        };
        let mut engine = ZoneEngine::from_catalog(&catalog);
        let track = make_track(1, "person", [100.0, 100.0, 200.0, 200.0], true);

        engine.evaluate_at(&[&track], Instant::now()); // cfg(test)
        let events = engine.evaluate_at(&[], Instant::now()); // cfg(test)
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ZoneEvent::Vacated { zone, .. } if zone == "bed"))
        );
    }

    #[test]
    fn non_confirmed_track_ignored() {
        let catalog = ZoneCatalog {
            zones: HashMap::from([(
                "bed".into(),
                ZoneSpec {
                    x1: 0,
                    y1: 0,
                    x2: 500,
                    y2: 500,
                    label: None,
                    hysteresis_ms: 100,
                },
            )]),
            face_dwell: None,
        };
        let mut engine = ZoneEngine::from_catalog(&catalog);
        let unconfirmed = make_track(1, "person", [100.0, 100.0, 200.0, 200.0], false);
        let events = engine.evaluate_at(&[&unconfirmed], Instant::now()); // cfg(test)
        assert!(events.is_empty());
    }

    #[test]
    fn vacate_waits_for_hysteresis_ms() {
        let catalog = ZoneCatalog {
            zones: HashMap::from([(
                "bed".into(),
                ZoneSpec {
                    x1: 0,
                    y1: 0,
                    x2: 500,
                    y2: 500,
                    label: None,
                    hysteresis_ms: 500,
                },
            )]),
            face_dwell: None,
        };
        let start = Instant::now(); // cfg(test)
        let mut engine = ZoneEngine::from_catalog(&catalog);
        let track = make_track(1, "person", [100.0, 100.0, 200.0, 200.0], true);

        let occupied = engine.evaluate_at(&[&track], start);
        assert!(
            occupied
                .iter()
                .any(|e| matches!(e, ZoneEvent::Occupied { zone, .. } if zone == "bed"))
        );

        let early = engine.evaluate_at(&[], start);
        assert!(
            early.is_empty(),
            "vacate must wait for hysteresis; got {early:?}"
        );
        let still_early =
            engine.evaluate_at(&[], start + std::time::Duration::from_millis(499));
        assert!(
            still_early.is_empty(),
            "vacate must wait full hysteresis; got {still_early:?}"
        );

        let vacated = engine.evaluate_at(&[], start + std::time::Duration::from_millis(500));
        assert!(
            vacated
                .iter()
                .any(|e| matches!(e, ZoneEvent::Vacated { zone, .. } if zone == "bed"))
        );
    }
}
