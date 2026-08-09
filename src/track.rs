use std::collections::HashMap;

use crate::assignment::hungarian_min;
use crate::detection::{ConsolidatedObservation, DetectionEvidence};
use crate::kalman::{Kalman7, KalmanConfig};
use crate::logger::Event;

/// Per-track state: position, motion model, lifecycle.
#[derive(Debug, Clone)]
pub struct Track {
    pub id: u64,
    pub source_model: String,
    pub class: String,
    pub bbox: [f32; 4],
    pub confidence: f32,
    pub evidence: Vec<DetectionEvidence>,
    pub(crate) kalman: Kalman7,
    pub hits: u32,
    pub hit_streak: u32,
    pub misses: u32,
    pub age: u32,
    pub time_since_update_ms: u64,
    pub is_confirmed: bool,
}

#[derive(Debug, Clone)]
pub enum TrackEvent {
    Created {
        id: u64,
        class: String,
        bbox: [f32; 4],
    },
    Updated {
        id: u64,
        bbox: [f32; 4],
    },
    Lost {
        id: u64,
        class: String,
        misses: u32,
    },
    Deleted {
        id: u64,
        class: String,
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct TrackObservation {
    pub primary_model: String,
    pub class: String,
    pub confidence: f32,
    pub bbox: [f32; 4],
    pub evidence: Vec<DetectionEvidence>,
}

impl From<&ConsolidatedObservation> for TrackObservation {
    fn from(observation: &ConsolidatedObservation) -> Self {
        Self {
            primary_model: observation.primary_model.clone(),
            class: observation.class.clone(),
            confidence: observation.confidence,
            bbox: observation.bbox,
            evidence: observation
                .evidence
                .iter()
                .chain(observation.components.iter())
                .cloned()
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TrackerConfig {
    pub min_hits: u32,
    pub max_age_ms: u64,
    pub tentative_max_age_ms: u64,
    pub iou_threshold: f32,
    pub ghost_max_ms: u64,
    pub nominal_dt_ms: u64,
    pub measurement_noise: f32,
    pub process_position_noise: f32,
    pub process_velocity_noise: f32,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            min_hits: 2,
            max_age_ms: 4_000,
            tentative_max_age_ms: 600,
            iou_threshold: 0.2,
            ghost_max_ms: 6_000,
            nominal_dt_ms: 2_000,
            measurement_noise: 1.0,
            process_position_noise: 1.0,
            process_velocity_noise: 0.25,
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn kalman_config(config: &TrackerConfig) -> KalmanConfig {
    KalmanConfig {
        nominal_dt_s: config.nominal_dt_ms as f32 / 1000.0,
        max_dt_s: config.ghost_max_ms as f32 / 1000.0,
        measurement_noise: config.measurement_noise,
        process_position_noise: config.process_position_noise,
        process_velocity_noise: config.process_velocity_noise,
    }
}

pub struct Tracker {
    tracks: HashMap<u64, Track>,
    next_id: u64,
    max_age_ms: u64,
    tentative_max_age_ms: u64,
    iou_threshold: f32,
    min_hits: u32,
    kalman_config: KalmanConfig,
}

impl Tracker {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::with_config(TrackerConfig::default())
    }

    pub fn with_config(config: TrackerConfig) -> Self {
        let kalman_config = kalman_config(&config);
        Self {
            tracks: HashMap::new(),
            next_id: 1,
            max_age_ms: config.max_age_ms,
            tentative_max_age_ms: config.tentative_max_age_ms,
            iou_threshold: config.iou_threshold,
            min_hits: config.min_hits.max(1),
            kalman_config,
        }
    }

    pub fn update(&mut self, detections: &[TrackObservation], dt_ms: u64) -> Vec<TrackEvent> {
        self.update_with_mode(detections, false, dt_ms)
    }

    fn update_with_mode(
        &mut self,
        detections: &[TrackObservation],
        allow_single_reacquire: bool,
        dt_ms: u64,
    ) -> Vec<TrackEvent> {
        let mut events = Vec::new();

        self.predict_all(dt_ms);
        let (det_matched, track_matched) =
            self.match_detections(detections, allow_single_reacquire);

        self.update_matched(&track_matched, detections, &mut events);
        self.age_unmatched(&track_matched, dt_ms, &mut events);
        self.delete_expired(&mut events);
        self.create_tracks(&det_matched, detections, &mut events);

        events
    }

    pub fn update_observations(
        &mut self,
        observations: &[ConsolidatedObservation],
        dt_ms: u64,
    ) -> Vec<TrackEvent> {
        let track_observations: Vec<TrackObservation> =
            observations.iter().map(TrackObservation::from).collect();
        self.update_with_mode(&track_observations, false, dt_ms)
    }

    /// Reacquire the existing confirmed identity when the room is expected to
    /// contain one person. This avoids spawning a new ID after a brief bbox
    /// jump or detector dropout.
    pub fn update_single_person(
        &mut self,
        observations: &[ConsolidatedObservation],
        dt_ms: u64,
    ) -> Vec<TrackEvent> {
        let track_observations: Vec<TrackObservation> =
            observations.iter().map(TrackObservation::from).collect();
        self.update_with_mode(&track_observations, true, dt_ms)
    }

    pub fn enrich_observations(&mut self, observations: &[ConsolidatedObservation]) {
        for observation in observations {
            let Some(track_id) = self
                .tracks
                .iter()
                .filter(|(_, track)| track.is_confirmed && track.class == observation.class)
                .filter_map(|(id, track)| {
                    let iou = compute_iou(&track.bbox, &observation.bbox);
                    (iou >= self.iou_threshold).then_some((*id, iou))
                })
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(id, _)| id)
            else {
                continue;
            };
            let track = self
                .tracks
                .get_mut(&track_id)
                .expect("track selected above");
            track.confidence = track.confidence.max(observation.confidence);
            let evidence: Vec<DetectionEvidence> = observation
                .evidence
                .iter()
                .chain(observation.components.iter())
                .cloned()
                .collect();
            merge_evidence(&mut track.evidence, &evidence);
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn predict_all(&mut self, dt_ms: u64) {
        for track in self.tracks.values_mut() {
            track.kalman.predict(dt_ms as f32 / 1000.0);
            track.bbox = track.kalman.bbox();
        }
    }

    fn match_detections(
        &self,
        detections: &[TrackObservation],
        allow_single_reacquire: bool,
    ) -> (Vec<bool>, HashMap<u64, usize>) {
        let track_ids: Vec<u64> = self.tracks.keys().copied().collect();
        let mut cost = vec![vec![f32::INFINITY; detections.len()]; track_ids.len()];
        for (row, &track_id) in track_ids.iter().enumerate() {
            let track = &self.tracks[&track_id];
            for (col, detection) in detections.iter().enumerate() {
                if track.class != detection.class {
                    continue;
                }
                cost[row][col] = 1.0 - compute_iou(&track.bbox, &detection.bbox);
            }
        }

        let max_cost = 1.0 - self.iou_threshold;
        let (matched, _, _) = hungarian_min(&cost, max_cost);

        let mut det_matched = vec![false; detections.len()];
        let mut track_matched: HashMap<u64, usize> = HashMap::new();
        for (row, col) in matched {
            track_matched.insert(track_ids[row], col);
            det_matched[col] = true;
        }

        if allow_single_reacquire && detections.len() == 1 && !det_matched[0] {
            let detection = &detections[0];
            let best = self
                .tracks
                .values()
                .filter(|track| track.is_confirmed && track.class == detection.class)
                .max_by(|a, b| {
                    compute_iou(&a.bbox, &detection.bbox)
                        .partial_cmp(&compute_iou(&b.bbox, &detection.bbox))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| b.misses.cmp(&a.misses))
                });
            if let Some(track) = best {
                track_matched.insert(track.id, 0);
                det_matched[0] = true;
            }
        }

        (det_matched, track_matched)
    }

    fn update_matched(
        &mut self,
        track_matched: &HashMap<u64, usize>,
        detections: &[TrackObservation],
        events: &mut Vec<TrackEvent>,
    ) {
        for (&track_id, &det_idx) in track_matched {
            let track = self
                .tracks
                .get_mut(&track_id)
                .expect("track must exist after match");
            let observation = &detections[det_idx];
            track.kalman.update(bbox_to_measurement(observation.bbox));
            track.bbox = track.kalman.bbox();
            track.confidence = observation.confidence;
            track.hits += 1;
            track.hit_streak += 1;
            track.misses = 0;
            track.time_since_update_ms = 0;
            track.age += 1;
            if track.hit_streak >= self.min_hits {
                track.is_confirmed = true;
            }
            merge_evidence(&mut track.evidence, &observation.evidence);
            events.push(TrackEvent::Updated {
                id: track_id,
                bbox: track.bbox,
            });
        }
    }

    fn create_tracks(
        &mut self,
        det_matched: &[bool],
        detections: &[TrackObservation],
        events: &mut Vec<TrackEvent>,
    ) {
        for (i, matched) in det_matched.iter().enumerate() {
            if *matched {
                continue;
            }
            let observation = &detections[i];
            let id = self.next_id;
            self.next_id += 1;
            self.tracks.insert(
                id,
                Track {
                    id,
                    source_model: observation.primary_model.clone(),
                    class: observation.class.clone(),
                    bbox: observation.bbox,
                    confidence: observation.confidence,
                    evidence: observation.evidence.clone(),
                    kalman: Kalman7::from_bbox_with_config(observation.bbox, self.kalman_config),
                    hits: 1,
                    hit_streak: 1,
                    misses: 0,
                    age: 1,
                    time_since_update_ms: 0,
                    is_confirmed: false,
                },
            );
            events.push(TrackEvent::Created {
                id,
                class: observation.class.clone(),
                bbox: observation.bbox,
            });
        }
    }

    fn age_unmatched(
        &mut self,
        track_matched: &HashMap<u64, usize>,
        dt_ms: u64,
        events: &mut Vec<TrackEvent>,
    ) {
        for track in self.tracks.values_mut() {
            if track_matched.contains_key(&track.id) {
                continue;
            }
            track.misses += 1;
            track.time_since_update_ms += dt_ms;
            track.hit_streak = 0;
            track.age += 1;
            if track.is_confirmed {
                events.push(TrackEvent::Lost {
                    id: track.id,
                    class: track.class.clone(),
                    misses: track.misses,
                });
            }
        }
    }

    fn delete_expired(&mut self, events: &mut Vec<TrackEvent>) {
        let to_delete: Vec<u64> = self
            .tracks
            .iter()
            .filter(|(_, t)| {
                (t.is_confirmed && t.time_since_update_ms > self.max_age_ms)
                    || (!t.is_confirmed && t.time_since_update_ms > self.tentative_max_age_ms)
            })
            .map(|(id, _)| *id)
            .collect();

        for id in &to_delete {
            if let Some(track) = self.tracks.remove(id) {
                let reason = if track.is_confirmed {
                    "age_exceeded"
                } else {
                    "unconfirmed"
                };
                events.push(TrackEvent::Deleted {
                    id: *id,
                    class: track.class,
                    reason: reason.to_string(),
                });
            }
        }
    }

    pub fn active_tracks(&self) -> Vec<&Track> {
        self.tracks.values().filter(|t| t.is_confirmed).collect()
    }

    pub fn current_tracks(&self) -> Vec<&Track> {
        self.tracks
            .values()
            .filter(|t| t.is_confirmed && t.misses == 0)
            .collect()
    }

    #[allow(dead_code)]
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }
}

fn merge_evidence(target: &mut Vec<DetectionEvidence>, incoming: &[DetectionEvidence]) {
    for evidence in incoming {
        if let Some(existing) = target.iter_mut().find(|item| item.model == evidence.model) {
            *existing = evidence.clone();
        } else {
            target.push(evidence.clone());
        }
    }
}

fn bbox_to_measurement(bbox: [f32; 4]) -> [f32; 4] {
    let [x1, y1, x2, y2] = bbox;
    let w = (x2 - x1).max(1e-3);
    let h = (y2 - y1).max(1e-3);
    [(x1 + x2) * 0.5, (y1 + y2) * 0.5, w * h, w / h]
}

/// Intersection over Union for axis-aligned bounding boxes.
fn compute_iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let ix1 = a[0].max(b[0]);
    let iy1 = a[1].max(b[1]);
    let ix2 = a[2].min(b[2]);
    let iy2 = a[3].min(b[3]);

    let iw = (ix2 - ix1).max(0.0);
    let ih = (iy2 - iy1).max(0.0);
    let inter = iw * ih;

    let area_a = (a[2] - a[0]) * (a[3] - a[1]);
    let area_b = (b[2] - b[0]) * (b[3] - b[1]);
    let union = area_a + area_b - inter;

    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Convert tracker events to logger events.
pub fn track_event_to_log(event: &TrackEvent, frame_id: u64) -> Event {
    match event {
        TrackEvent::Created { id, class, bbox } => Event::Meta {
            event: "track_created".into(),
            detail: format!("track{id}"),
            attrs: vec![
                ("class".into(), class.clone()),
                ("frame_id".into(), frame_id.to_string()),
                ("bbox".into(), format_bbox(bbox)),
            ],
        },
        TrackEvent::Updated { id, bbox } => Event::Meta {
            event: "track_updated".into(),
            detail: format!("track{id}"),
            attrs: vec![
                ("frame_id".into(), frame_id.to_string()),
                ("bbox".into(), format_bbox(bbox)),
            ],
        },
        TrackEvent::Lost { id, class, misses } => Event::Meta {
            event: "track_lost".into(),
            detail: format!("track{id}"),
            attrs: vec![
                ("class".into(), class.clone()),
                ("misses".into(), misses.to_string()),
                ("frame_id".into(), frame_id.to_string()),
            ],
        },
        TrackEvent::Deleted { id, class, reason } => Event::Meta {
            event: "track_deleted".into(),
            detail: format!("track{id}"),
            attrs: vec![
                ("class".into(), class.clone()),
                ("reason".into(), reason.clone()),
                ("frame_id".into(), frame_id.to_string()),
            ],
        },
    }
}

fn format_bbox(bbox: &[f32; 4]) -> String {
    format!(
        "[{:.0},{:.0},{:.0},{:.0}]",
        bbox[0], bbox[1], bbox[2], bbox[3]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT_MS: u64 = 200;

    fn observation(class: &str, bbox: [f32; 4]) -> TrackObservation {
        TrackObservation {
            primary_model: "detect-fast".into(),
            class: class.into(),
            confidence: 0.9,
            bbox,
            evidence: Vec::new(),
        }
    }

    fn consolidated(bbox: [f32; 4]) -> crate::detection::ConsolidatedObservation {
        crate::detection::ConsolidatedObservation {
            class: "person".into(),
            confidence: 0.9,
            bbox,
            primary_model: "detect-fast".into(),
            evidence: Vec::new(),
            components: Vec::new(),
        }
    }

    #[test]
    fn single_detection_creates_track() {
        let mut tracker = Tracker::new();
        let dets = vec![observation("person", [100.0, 200.0, 300.0, 500.0])];
        let events = tracker.update(&dets, DT_MS);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, TrackEvent::Created { .. }))
        );
        assert_eq!(tracker.track_count(), 1);
    }

    #[test]
    fn consecutive_detections_maintain_track_id() {
        let mut tracker = Tracker::new();
        let dets1 = vec![observation("person", [100.0, 200.0, 300.0, 500.0])];
        tracker.update(&dets1, DT_MS);

        let dets2 = vec![observation("person", [105.0, 205.0, 305.0, 505.0])];
        let events = tracker.update(&dets2, DT_MS);

        assert!(
            events
                .iter()
                .any(|e| matches!(e, TrackEvent::Updated { id: 1, .. }))
        );
        assert_eq!(tracker.track_count(), 1);
    }

    #[test]
    fn missing_detection_marks_track_lost() {
        let mut tracker = Tracker::new();
        let dets = vec![observation("person", [100.0, 200.0, 300.0, 500.0])];
        tracker.update(&dets, DT_MS);
        // second hit to confirm
        tracker.update(&dets, DT_MS);

        // no detections this frame
        let events = tracker.update(&[], DT_MS);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, TrackEvent::Lost { id: 1, .. }))
        );
    }

    #[test]
    fn iou_matching_handles_overlap() {
        let iou = compute_iou(&[0.0, 0.0, 100.0, 100.0], &[50.0, 50.0, 150.0, 150.0]);
        assert!((iou - 0.142).abs() < 0.01, "expected IoU ~0.14, got {iou}");
    }

    #[test]
    fn non_overlapping_boxes_iou_zero() {
        let iou = compute_iou(&[0.0, 0.0, 50.0, 50.0], &[100.0, 100.0, 150.0, 150.0]);
        assert_eq!(iou, 0.0);
    }

    #[test]
    fn identical_boxes_iou_one() {
        let iou = compute_iou(&[10.0, 20.0, 110.0, 220.0], &[10.0, 20.0, 110.0, 220.0]);
        assert!((iou - 1.0).abs() < 0.001);
    }

    #[test]
    fn high_iou_matches_across_small_displacement() {
        let mut tracker = Tracker::new();
        tracker.update(&[observation("person", [100.0, 200.0, 300.0, 500.0])], DT_MS);
        tracker.update(&[observation("person", [100.0, 200.0, 300.0, 500.0])], DT_MS);

        let events = tracker.update(
            &[observation("person", [110.0, 210.0, 310.0, 510.0])],
            DT_MS,
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, TrackEvent::Updated { id: 1, .. }))
        );
        assert_eq!(tracker.track_count(), 1);
    }

    #[test]
    fn single_person_reacquire_keeps_identity_after_large_bbox_jump() {
        let mut tracker = Tracker::new();
        let first = consolidated([100.0, 100.0, 200.0, 300.0]);
        let jumped = consolidated([500.0, 100.0, 600.0, 300.0]);

        tracker.update_single_person(std::slice::from_ref(&first), DT_MS);
        tracker.update_single_person(std::slice::from_ref(&first), DT_MS);
        let events = tracker.update_single_person(std::slice::from_ref(&jumped), DT_MS);

        assert!(
            events
                .iter()
                .any(|event| matches!(event, TrackEvent::Updated { id: 1, .. }))
        );
        assert_eq!(tracker.current_tracks().len(), 1);
        assert_eq!(tracker.current_tracks()[0].id, 1);
        assert_eq!(tracker.track_count(), 1);
    }

    #[test]
    fn class_mismatch_does_not_reuse_track() {
        let mut tracker = Tracker::new();
        tracker.update(&[observation("person", [100.0, 100.0, 200.0, 300.0])], DT_MS);
        let events = tracker.update(&[observation("chair", [100.0, 100.0, 200.0, 300.0])], DT_MS);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, TrackEvent::Created { id: 2, class, .. } if class == "chair"))
        );
        assert_eq!(tracker.track_count(), 2);
    }

    #[test]
    fn current_tracks_excludes_missing_confirmed_tracks() {
        let mut tracker = Tracker::new();
        let det = observation("person", [100.0, 100.0, 200.0, 300.0]);
        tracker.update(std::slice::from_ref(&det), DT_MS);
        tracker.update(std::slice::from_ref(&det), DT_MS);
        assert_eq!(tracker.current_tracks().len(), 1);
        tracker.update(&[], DT_MS);
        assert!(tracker.current_tracks().is_empty());
        assert_eq!(tracker.active_tracks().len(), 1);
    }

    #[test]
    fn tentative_track_survives_short_detection_gap() {
        let mut tracker = Tracker::with_config(TrackerConfig {
            min_hits: 2,
            max_age_ms: 4_000,
            tentative_max_age_ms: 600,
            iou_threshold: 0.2,
            ..TrackerConfig::default()
        });
        let det = observation("person", [100.0, 100.0, 200.0, 300.0]);

        tracker.update(std::slice::from_ref(&det), DT_MS);
        tracker.update(&[], DT_MS);
        tracker.update(&[], DT_MS);
        let events = tracker.update(std::slice::from_ref(&det), DT_MS);

        assert!(
            events
                .iter()
                .any(|event| matches!(event, TrackEvent::Updated { id: 1, .. }))
        );
        assert_eq!(tracker.track_count(), 1);

        tracker.update(std::slice::from_ref(&det), DT_MS);
        assert_eq!(tracker.current_tracks()[0].id, 1);
    }

    #[test]
    fn tentative_track_expires_after_configured_age() {
        let mut tracker = Tracker::with_config(TrackerConfig {
            min_hits: 2,
            max_age_ms: 4_000,
            tentative_max_age_ms: 300,
            iou_threshold: 0.2,
            ..TrackerConfig::default()
        });
        tracker.update(&[observation("person", [100.0, 100.0, 200.0, 300.0])], DT_MS);
        tracker.update(&[], DT_MS);
        let events = tracker.update(&[], DT_MS);

        assert!(events.iter().any(|event| matches!(
            event,
            TrackEvent::Deleted { id: 1, reason, .. } if reason == "unconfirmed"
        )));
        assert_eq!(tracker.track_count(), 0);
    }

    #[test]
    fn uniform_keyframe_cadence_preserves_identity() {
        let mut tracker = Tracker::new();
        // El ancho de 200px conserva IoU > 0.2 entre muestras a 2 s: el test
        // aísla el stall sin convertir la asociación inicial en otro caso.
        let at = |x: f32| observation("person", [x, 100.0, x + 200.0, 300.0]);
        // 50 px/s a cadencia real uniforme de 2 s.
        tracker.update(&[at(100.0)], 2_000);
        tracker.update(&[at(200.0)], 2_000);
        let id = tracker.current_tracks()[0].id;
        let events = tracker.update(&[at(300.0)], 2_000);
        assert!(!events.iter().any(|e| matches!(e, TrackEvent::Created { .. })));
        assert_eq!(tracker.current_tracks()[0].id, id);
    }

    #[test]
    fn network_stall_does_not_switch_identity() {
        let mut tracker = Tracker::new();
        // El ancho de 200px mantiene IoU > 0.2 para aprender la velocidad
        // antes del stall; el gap largo se prueba contra la predicción.
        let at = |x: f32| observation("person", [x, 100.0, x + 200.0, 300.0]);
        tracker.update(&[at(100.0)], 2_000);
        tracker.update(&[at(200.0)], 2_000);
        let id = tracker.current_tracks()[0].id;
        // Stall de 6 s: la persona avanzó 300 px proporcionalmente al tiempo.
        let events = tracker.update(&[at(500.0)], 6_000);
        assert!(!events.iter().any(|e| matches!(e, TrackEvent::Created { .. })));
        assert_eq!(tracker.current_tracks()[0].id, id);
    }

    #[test]
    fn optimal_matching_avoids_identity_split_that_greedy_would_cause() {
        let mut tracker = Tracker::new();
        let a = observation("person", [0.0, 0.0, 100.0, 100.0]);
        let b = observation("person", [80.0, 0.0, 180.0, 100.0]);
        tracker.update(&[a.clone(), b.clone()], DT_MS);
        tracker.update(&[a, b], DT_MS);

        // Frame conflictivo: d_high (confianza alta) superpone a y b (0.667
        // vs 0.25); d_low (confianza baja) solo superpone a (1.0). El greedy
        // por confianza tomaria d_high->a y dejaria d_low sin track
        // (identidad partida); el hungaro asigna d_high->b y d_low->a.
        let mut d_high = observation("person", [20.0, 0.0, 120.0, 100.0]);
        d_high.confidence = 0.95;
        let mut d_low = observation("person", [0.0, 0.0, 100.0, 100.0]);
        d_low.confidence = 0.3;

        let events = tracker.update(&[d_high, d_low], DT_MS);
        assert_eq!(tracker.track_count(), 2, "no identity split");
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, TrackEvent::Created { .. })),
            "no new track created"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, TrackEvent::Lost { .. })),
            "no track lost"
        );
    }

    #[test]
    fn kalman_smooths_jittery_detections() {
        let mut tracker = Tracker::new();
        let jittered =
            |dx: f32| observation("person", [100.0 + dx, 100.0 + dx, 200.0 + dx, 300.0 + dx]);
        tracker.update(&[jittered(0.0)], DT_MS);
        tracker.update(&[jittered(4.0)], DT_MS);
        tracker.update(&[jittered(-3.0)], DT_MS);
        tracker.update(&[jittered(2.0)], DT_MS);
        tracker.update(&[jittered(-1.0)], DT_MS);

        let track = tracker.current_tracks()[0];
        let bbox = track.bbox;
        assert!(
            (bbox[0] - 100.0).abs() < 3.0,
            "smoothed x1 should stay near the mean: {}",
            bbox[0]
        );
        assert!(
            (bbox[2] - 200.0).abs() < 3.0,
            "smoothed x2 should stay near the mean: {}",
            bbox[2]
        );
    }
}
