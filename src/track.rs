use std::collections::HashMap;

use crate::detection::{ConsolidatedObservation, DetectionEvidence};
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
    pub(crate) velocity: [f32; 4],
    pub hits: u32,
    pub hit_streak: u32,
    pub misses: u32,
    pub age: u32,
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
    pub max_age: u32,
    pub tentative_max_age: u32,
    pub iou_threshold: f32,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            min_hits: 2,
            max_age: 20,
            tentative_max_age: 3,
            iou_threshold: 0.2,
        }
    }
}

pub struct Tracker {
    tracks: HashMap<u64, Track>,
    next_id: u64,
    max_age: u32,
    tentative_max_age: u32,
    iou_threshold: f32,
    min_hits: u32,
}

impl Tracker {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::with_config(TrackerConfig::default())
    }

    pub fn with_config(config: TrackerConfig) -> Self {
        Self {
            tracks: HashMap::new(),
            next_id: 1,
            max_age: config.max_age,
            tentative_max_age: config.tentative_max_age,
            iou_threshold: config.iou_threshold,
            min_hits: config.min_hits.max(1),
        }
    }

    pub fn update(&mut self, detections: &[TrackObservation]) -> Vec<TrackEvent> {
        let mut events = Vec::new();

        self.predict_all();
        let (det_matched, track_matched) = self.match_detections(detections);

        self.update_matched(&track_matched, detections, &mut events);
        self.age_unmatched(&track_matched, &mut events);
        self.delete_expired(&mut events);
        self.create_tracks(&det_matched, detections, &mut events);

        events
    }

    pub fn update_observations(
        &mut self,
        observations: &[ConsolidatedObservation],
    ) -> Vec<TrackEvent> {
        let track_observations: Vec<TrackObservation> =
            observations.iter().map(TrackObservation::from).collect();
        self.update(&track_observations)
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

    fn predict_all(&mut self) {
        for track in self.tracks.values_mut() {
            track.predict();
        }
    }

    fn match_detections(
        &self,
        detections: &[TrackObservation],
    ) -> (Vec<bool>, HashMap<u64, usize>) {
        let mut det_matched = vec![false; detections.len()];
        let mut track_matched: HashMap<u64, usize> = HashMap::new();

        let mut det_indices: Vec<usize> = (0..detections.len()).collect();
        det_indices.sort_by(|&a, &b| {
            detections[b]
                .confidence
                .partial_cmp(&detections[a].confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for &det_idx in &det_indices {
            if det_matched[det_idx] {
                continue;
            }
            let det_bbox = &detections[det_idx].bbox;

            let mut best_iou = self.iou_threshold;
            let mut best_track: Option<u64> = None;

            for (&track_id, track) in &self.tracks {
                if track_matched.contains_key(&track_id) {
                    continue;
                }
                if track.class != detections[det_idx].class {
                    continue;
                }
                let iou = compute_iou(&track.bbox, det_bbox);
                if iou > best_iou {
                    best_iou = iou;
                    best_track = Some(track_id);
                }
            }

            if let Some(track_id) = best_track {
                track_matched.insert(track_id, det_idx);
                det_matched[det_idx] = true;
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
            track.update_bbox(observation.bbox, self.min_hits);
            track.confidence = observation.confidence;
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
                    velocity: [0.0; 4],
                    hits: 1,
                    hit_streak: 1,
                    misses: 0,
                    age: 1,
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

    fn age_unmatched(&mut self, track_matched: &HashMap<u64, usize>, events: &mut Vec<TrackEvent>) {
        for track in self.tracks.values_mut() {
            if track_matched.contains_key(&track.id) {
                continue;
            }
            track.misses += 1;
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
                (t.is_confirmed && t.misses > self.max_age)
                    || (!t.is_confirmed && t.misses > self.tentative_max_age)
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

impl Track {
    /// Predict next position using simple velocity model.
    fn predict(&mut self) {
        for i in 0..4 {
            self.bbox[i] += self.velocity[i];
        }
        // Clamp to avoid runaway
        for i in 0..4 {
            self.bbox[i] = self.bbox[i].clamp(0.0, 10_000.0);
        }
    }

    /// Update position and recompute velocity from displacement.
    fn update_bbox(&mut self, new_bbox: [f32; 4], min_hits: u32) {
        let prev = self.bbox;
        self.bbox = new_bbox;
        for i in 0..4 {
            self.velocity[i] = new_bbox[i] - prev[i];
        }
        self.hits += 1;
        self.hit_streak += 1;
        self.misses = 0;
        self.age += 1;
        if self.hit_streak >= min_hits {
            self.is_confirmed = true;
        }
    }
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

    fn observation(class: &str, bbox: [f32; 4]) -> TrackObservation {
        TrackObservation {
            primary_model: "detect-fast".into(),
            class: class.into(),
            confidence: 0.9,
            bbox,
            evidence: Vec::new(),
        }
    }

    #[test]
    fn single_detection_creates_track() {
        let mut tracker = Tracker::new();
        let dets = vec![observation("person", [100.0, 200.0, 300.0, 500.0])];
        let events = tracker.update(&dets);
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
        tracker.update(&dets1);

        let dets2 = vec![observation("person", [105.0, 205.0, 305.0, 505.0])];
        let events = tracker.update(&dets2);

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
        tracker.update(&dets);
        // second hit to confirm
        tracker.update(&dets);

        // no detections this frame
        let events = tracker.update(&[]);
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
        tracker.update(&[observation("person", [100.0, 200.0, 300.0, 500.0])]);
        tracker.update(&[observation("person", [100.0, 200.0, 300.0, 500.0])]);

        let events = tracker.update(&[observation("person", [110.0, 210.0, 310.0, 510.0])]);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, TrackEvent::Updated { id: 1, .. }))
        );
        assert_eq!(tracker.track_count(), 1);
    }

    #[test]
    fn class_mismatch_does_not_reuse_track() {
        let mut tracker = Tracker::new();
        tracker.update(&[observation("person", [100.0, 100.0, 200.0, 300.0])]);
        let events = tracker.update(&[observation("chair", [100.0, 100.0, 200.0, 300.0])]);
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
        tracker.update(std::slice::from_ref(&det));
        tracker.update(std::slice::from_ref(&det));
        assert_eq!(tracker.current_tracks().len(), 1);
        tracker.update(&[]);
        assert!(tracker.current_tracks().is_empty());
        assert_eq!(tracker.active_tracks().len(), 1);
    }

    #[test]
    fn tentative_track_survives_short_detection_gap() {
        let mut tracker = Tracker::with_config(TrackerConfig {
            min_hits: 2,
            max_age: 20,
            tentative_max_age: 3,
            iou_threshold: 0.2,
        });
        let det = observation("person", [100.0, 100.0, 200.0, 300.0]);

        tracker.update(std::slice::from_ref(&det));
        tracker.update(&[]);
        tracker.update(&[]);
        let events = tracker.update(std::slice::from_ref(&det));

        assert!(
            events
                .iter()
                .any(|event| matches!(event, TrackEvent::Updated { id: 1, .. }))
        );
        assert_eq!(tracker.track_count(), 1);

        tracker.update(std::slice::from_ref(&det));
        assert_eq!(tracker.current_tracks()[0].id, 1);
    }

    #[test]
    fn tentative_track_expires_after_configured_age() {
        let mut tracker = Tracker::with_config(TrackerConfig {
            min_hits: 2,
            max_age: 20,
            tentative_max_age: 1,
            iou_threshold: 0.2,
        });
        tracker.update(&[observation("person", [100.0, 100.0, 200.0, 300.0])]);
        tracker.update(&[]);
        let events = tracker.update(&[]);

        assert!(events.iter().any(|event| matches!(
            event,
            TrackEvent::Deleted { id: 1, reason, .. } if reason == "unconfirmed"
        )));
        assert_eq!(tracker.track_count(), 0);
    }
}
