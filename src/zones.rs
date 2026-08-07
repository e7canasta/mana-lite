use std::collections::HashMap;
use std::time::Instant;

use crate::config::ZoneCatalog;
use crate::logger::Event;
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
    vacated_since: Option<Instant>,
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
                    vacated_since: None,
                },
            );
        }
        Self { zones }
    }

    pub fn evaluate(&mut self, tracks: &[&Track]) -> Vec<ZoneEvent> {
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
                    state.vacated_since = None;
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
                let vacated_at = state.vacated_since.get_or_insert(Instant::now());
                if vacated_at.elapsed().as_millis() as u64 >= state.hysteresis_ms {
                    state.is_occupied = false;
                    let prev = std::mem::take(&mut state.occupied_by);
                    state.vacated_since = None;
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
}

fn find_class_confidence(tracks: &[&Track], id: u64) -> (String, f32) {
    tracks
        .iter()
        .find(|t| t.id == id)
        .map(|t| (t.class.clone(), t.confidence))
        .unwrap_or_else(|| ("unknown".into(), 0.0))
}

fn rect_intersects(bbox: &[f32; 4], zone: &[f32; 4]) -> bool {
    bbox[0] < zone[2] && bbox[2] > zone[0] && bbox[1] < zone[3] && bbox[3] > zone[1]
}

pub fn zone_event_to_log(ev: &ZoneEvent, frame_id: u64) -> Event {
    match ev {
        ZoneEvent::Occupied {
            zone,
            label,
            track_id: _,
            class,
            confidence,
        } => Event::zone_occupied(
            zone,
            label.as_deref().unwrap_or(zone),
            class,
            *confidence,
            frame_id,
        ),
        ZoneEvent::Vacated {
            zone,
            label,
            track_id: _,
            class,
        } => Event::zone_vacated(zone, label.as_deref().unwrap_or(zone), class, frame_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ZoneEntry;

    fn make_track(id: u64, class: &str, bbox: [f32; 4], confirmed: bool) -> Track {
        Track {
            id,
            source_model: "detect-fast".into(),
            class: class.into(),
            bbox,
            confidence: 0.9,
            evidence: Vec::new(),
            velocity: [0.0; 4],
            hits: if confirmed { 3 } else { 1 },
            hit_streak: if confirmed { 3 } else { 1 },
            misses: 0,
            age: 3,
            is_confirmed: confirmed,
        }
    }

    #[test]
    fn occupied_triggers_event() {
        let catalog = ZoneCatalog {
            zones: HashMap::from([(
                "bed".into(),
                ZoneEntry {
                    x1: 0,
                    y1: 0,
                    x2: 200,
                    y2: 200,
                    label: None,
                    hysteresis_ms: 100,
                },
            )]),
        };
        let mut engine = ZoneEngine::from_catalog(&catalog);
        let track = make_track(1, "person", [50.0, 50.0, 150.0, 150.0], true);
        let events = engine.evaluate(&[&track]);
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
                ZoneEntry {
                    x1: 0,
                    y1: 0,
                    x2: 500,
                    y2: 500,
                    label: None,
                    hysteresis_ms: 0, // zero hysteresis for test
                },
            )]),
        };
        let mut engine = ZoneEngine::from_catalog(&catalog);
        let track = make_track(1, "person", [100.0, 100.0, 200.0, 200.0], true);

        engine.evaluate(&[&track]);
        let events = engine.evaluate(&[]);
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
                ZoneEntry {
                    x1: 0,
                    y1: 0,
                    x2: 500,
                    y2: 500,
                    label: None,
                    hysteresis_ms: 100,
                },
            )]),
        };
        let mut engine = ZoneEngine::from_catalog(&catalog);
        let unconfirmed = make_track(1, "person", [100.0, 100.0, 200.0, 200.0], false);
        let events = engine.evaluate(&[&unconfirmed]);
        assert!(events.is_empty());
    }
}
