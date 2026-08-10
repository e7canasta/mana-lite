//! Presence state and scalar / text metrics for Rerun viz.

use std::time::Instant;

use crate::metrics::PerClassFrameStats;
use crate::occupancy::{RoomCardinality, SecondPersonState, SignalValidity};

use super::{FRAME_NUMBER_TIMELINE, FRAME_TIME_TIMELINE, Inner, VizBridge, sanitize_entity_name};

impl VizBridge {
    pub fn set_frame_time(&self, frame_number: u64, timestamp_ns: i64) {
        if let Inner::Connected { ref rec, .. } = self.inner {
            let frame_number = i64::try_from(frame_number).unwrap_or(i64::MAX);
            rec.set_time_sequence(FRAME_NUMBER_TIMELINE, frame_number);
            rec.set_timestamp_nanos_since_epoch(FRAME_TIME_TIMELINE, timestamp_ns);
        }
    }

    pub fn log_occupancy_state(
        &mut self,
        state: RoomCardinality,
        second_person: SecondPersonState,
        signal: SignalValidity,
    ) {
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };

        if self.last_occupancy_state != Some(state) {
            let path = "/pipeline/state/room/cardinality";
            if let Err(e) = rec.log(path, &rerun::StateChange::single(state.as_str())) {
                log::warn!("viz occupancy state failed: {e}");
            }
            self.last_occupancy_state = Some(state);
        }
        if self.last_second_person_state != Some(second_person) {
            let path = "/pipeline/state/room/second_person";
            if let Err(e) = rec.log(path, &rerun::StateChange::single(second_person.as_str())) {
                log::warn!("viz second person state failed: {e}");
            }
            self.last_second_person_state = Some(second_person);
        }
        if self.last_signal_state != Some(signal) {
            let path = "/pipeline/state/room/signal";
            if let Err(e) = rec.log(path, &rerun::StateChange::single(signal.as_str())) {
                log::warn!("viz presence signal state failed: {e}");
            }
            self.last_signal_state = Some(signal);
        }
    }

    pub fn log_face_state(&mut self, state: &str) {
        if self.last_face_state.as_deref() == Some(state) {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        if let Err(e) = rec.log("/pipeline/state/face", &rerun::StateChange::single(state)) {
            log::warn!("viz face state failed: {e}");
        }
        self.last_face_state = Some(state.to_owned());
    }

    pub fn log_decode_latency(&self, us: u64) {
        if !self.toggles.decode_latency {
            return;
        }
        if let Inner::Connected { ref rec, .. } = self.inner {
            self.log_scalar_inner(rec, "/pipeline/decode/latency_us", us as f64);
        }
    }

    pub fn log_infer_latency(&mut self, model: &str, backend_us: u64, pipeline_us: u64) {
        let now = Instant::now();
        let infer_hz = self
            .last_infer_at
            .insert(model.to_owned(), now)
            .map(|previous| {
                let seconds = now.duration_since(previous).as_secs_f64();
                if seconds > 0.0 {
                    Some(1.0 / seconds)
                } else {
                    None
                }
            })
            .flatten();
        if let Inner::Connected { ref rec, .. } = self.inner {
            if self.toggles.infer_latency {
                let path = format!("/pipeline/infer/{model}/latency_us");
                self.log_scalar_inner(rec, &path, backend_us as f64);
                let path = format!("/pipeline/infer/{model}/pipeline_us");
                self.log_scalar_inner(rec, &path, pipeline_us as f64);
            }
            if self.toggles.infer_rate {
                if let Some(hz) = infer_hz {
                    let path = format!("/pipeline/infer/{model}/hz");
                    self.log_scalar_inner(rec, &path, hz);
                }
            }
        }
    }

    pub fn log_keyframe_gap(&self, dt_ms: u64) {
        if let Inner::Connected { ref rec, .. } = self.inner {
            if self.toggles.keyframe_gap {
                self.log_scalar_inner(rec, "/ingest/normal/gap_ms", dt_ms as f64);
            }
            if self.toggles.keyframe_rate && dt_ms > 0 {
                self.log_scalar_inner(rec, "/ingest/keyframes/processed_hz", 1000.0 / dt_ms as f64);
            }
        }
    }

    pub fn log_keyframe_selection(&self, seen: u64, dropped: u64, source_window_ms: u64) {
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        if self.toggles.keyframe_rate && source_window_ms > 0 {
            let source_hz = seen as f64 * 1000.0 / source_window_ms as f64;
            self.log_scalar_inner(rec, "/ingest/keyframes/source_hz", source_hz);
        }
        if self.toggles.keyframe_drops {
            self.log_scalar_inner(rec, "/ingest/keyframes/dropped", dropped as f64);
        }
    }

    pub fn log_per_frame_class_stats(&self, model: &str, per_class: &PerClassFrameStats) {
        if !self.toggles.class_counts_per_frame
            && !self.toggles.class_confidence_per_frame
            && !self.toggles.class_area_per_frame
        {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let model_safe = model.replace('-', "_").replace('.', "_");
        for (class, stat) in &per_class.stats {
            let cls_safe = sanitize_entity_name(class);
            if self.toggles.class_counts_per_frame {
                let path = format!("/infer/{model_safe}/per_frame/counts/{cls_safe}");
                self.log_scalar_inner(rec, &path, stat.count as f64);
            }
            if self.toggles.class_confidence_per_frame {
                let path_min = format!("/infer/{model_safe}/per_frame/conf/{cls_safe}/min");
                let path_max = format!("/infer/{model_safe}/per_frame/conf/{cls_safe}/max");
                self.log_scalar_inner(rec, &path_min, stat.conf_min as f64);
                self.log_scalar_inner(rec, &path_max, stat.conf_max as f64);
            }
            if self.toggles.class_area_per_frame {
                let path_min = format!("/infer/{model_safe}/per_frame/area/{cls_safe}/min");
                let path_max = format!("/infer/{model_safe}/per_frame/area/{cls_safe}/max");
                self.log_scalar_inner(rec, &path_min, stat.area_min);
                self.log_scalar_inner(rec, &path_max, stat.area_max);
            }
        }
    }

    pub(crate) fn log_scalar_inner(&self, rec: &rerun::RecordingStream, path: &str, value: f64) {
        if let Err(e) = rec.log(path, &rerun::Scalars::single(value)) {
            log::warn!("viz scalar {path} failed: {e}");
        }
    }
}
