//! Inference-cycle stages: schedule models, consolidate, record, publish.

use crate::cascade::{CascadeTarget, GateObservation, InferenceRequest};
use crate::config::CropType;
use crate::detection::CropRect;
use crate::detection::{ConsolidatedObservation, DetectionRole, ModelDetections};
use crate::infer::{InferenceResult, compute_bbox_roi, compute_upper_square_roi};
use crate::logger::Event;
use crate::snapshot::FrameBuffer;
use mana_perception::domain::{ClassName, ModelId};
use std::time::{Duration, Instant};

use super::body_parts::{
    ActorRef, BodyGeometry, BodyPartsEstimate, BodyPartsEstimator, PendingBodyPartsEvidence,
    attach_depth, attach_surface_evidence,
};
use super::cross_model_validation::{
    CrossModelValidation, EvidenceKind, PendingEvidence, validate_pending,
};
use super::face_pose::{
    PendingFacePoseContext, is_uncertain_face, select_pose_detection, validate_face_pose,
};
use super::perception::{PerceptionConfig, PerceptionStage};
use super::{CycleContext, FrameSize};

#[derive(Debug, Clone)]
struct ClinicalSample {
    observations: Vec<ConsolidatedObservation>,
    signal_valid: bool,
    raw_person_count: usize,
    frame_number: u64,
    face_model_ran: bool,
    face_pose_validation: Option<mana_control::FacePoseValidation>,
}

struct PendingModelOutput {
    model_key: String,
    output: InferenceResult,
    target: Option<CascadeTarget>,
    crop_frame: Option<crate::infer::CropFrameInfo>,
    crop_rect: Option<CropRect>,
}

impl PerceptionStage {
    /// Runs one inference cycle over a keyframe, in fixed stage order.
    pub(super) fn run_inference(&mut self, cycle: CycleContext<'_>, config: &PerceptionConfig) {
        let fb = cycle.frame;
        self.expire_face_pose_context(cycle.now);
        #[cfg(feature = "rerun")]
        self.observer.viz.clear_depth_context_boxes();

        let urgent_window = self.cascade.begin_keyframe(cycle.now);
        for request in &urgent_window.expired {
            self.lock_metrics()
                .tick_infer_urgent_expired(&request.model_key);
        }
        for request in &urgent_window.starved {
            self.lock_metrics()
                .tick_urgent_starvation(&request.model_key);
        }
        let urgent_request = urgent_window.requests.first().cloned();
        let mut requested = self.resolve_models(config);
        for request in &urgent_window.requests {
            if !requested.iter().any(|model| model == &request.model_key) {
                requested.push(request.model_key.clone());
            }
        }
        let ordered = self.cascade.ordered(&requested);
        self.count_models_gated_by_state(&ordered, config, cycle.now);
        let mut pending: Vec<PendingModelOutput> = Vec::new();

        let primary_root_valid = self.run_root_models(
            &ordered,
            fb,
            &mut pending,
            cycle.now,
            urgent_request.as_ref(),
        );
        self.run_child_models(
            &ordered,
            fb,
            &mut pending,
            cycle.now,
            urgent_request.as_ref(),
        );

        let cross_model_validations = self.validate_cross_model_from_pending(
            &pending,
            cycle.frame_number,
            fb.w,
            fb.h,
            &config.perception.validation,
            &config.face_pose,
        );
        for validation in &cross_model_validations {
            self.observer.emit(Event::cross_model_validation(
                validation.actor_id,
                validation.frame_number,
                validation.quality,
                validation.agreement,
                validation.freshness,
                validation.supporting_sources.clone(),
                validation.contradicting_sources.clone(),
                validation.reasons.clone(),
            ));
        }
        let mut body_parts = self.estimate_body_parts_from_pending(
            &pending,
            &cross_model_validations,
            cycle.frame_number,
            fb.w,
            fb.h,
            &config.perception.body_parts,
        );
        self.attach_body_parts_depth(&mut body_parts, &pending, fb.w, fb.h);
        self.attach_body_parts_surface_evidence(&mut body_parts, &pending, fb.w, fb.h);
        #[cfg(feature = "rerun")]
        self.observer.viz.log_body_parts(&body_parts);
        for estimate in &body_parts {
            self.observer.emit(body_parts_event(estimate));
        }

        let face_pose_validation =
            self.validate_face_pose_from_pending(&pending, cycle.frame_number, fb.w, fb.h, config);
        self.request_face_pose_from_pending(&pending, cycle.frame_number, config, cycle.now);
        let observations = self.consolidate_and_emit(&pending, cycle.frame_number);
        let face_model_ran = pending
            .iter()
            .any(|item| self.models.is_face_model(&item.model_key));
        let frame = FrameSize::new(fb.w, fb.h);
        #[cfg(feature = "rerun")]
        self.observer
            .viz
            .log_consolidated_observations(&observations, frame);

        self.record_pending_results(pending, frame, cycle.now);
        let raw_person_count = observations_person_count(&observations, &config.presence_class);
        self.publish_clinical_sample(
            ClinicalSample {
                observations,
                signal_valid: primary_root_valid,
                raw_person_count,
                frame_number: cycle.frame_number,
                face_model_ran,
                face_pose_validation,
            },
            cycle.now,
        );
    }

    /// Stage: run cascade roots (no parent) and note primary-root validity.
    fn run_root_models(
        &mut self,
        ordered: &[&str],
        fb: &FrameBuffer,
        pending: &mut Vec<PendingModelOutput>,
        now: Instant,
        urgent_request: Option<&InferenceRequest>,
    ) -> bool {
        let roots: Vec<String> = ordered
            .iter()
            .filter(|model| self.cascade.parent_of(model).is_none())
            .map(|model| (*model).to_owned())
            .collect();
        let mut primary_root_valid = false;
        for model_key in roots {
            let urgent = urgent_request.filter(|request| request.model_key == model_key);
            if urgent.is_none() && !self.cascade.is_due(&model_key, now) {
                let interval = self.cascade.interval_min_ms(&model_key).unwrap_or(0);
                let mut metrics = self.lock_metrics();
                metrics.set_model_interval(&model_key, interval);
                metrics.tick_infer_not_due(&model_key);
                continue;
            }
            let valid = self.run_scheduled_model(&model_key, None, fb, pending, now, urgent);
            if model_key == self.primary_model {
                primary_root_valid = valid;
            }
        }
        primary_root_valid
    }

    /// Stage: run cascade children whose declared rule resuelve un recorte.
    ///
    /// No hay compuerta acá. La condición —clase, cantidad exacta, confianza
    /// mínima, región— la declara el blueprint y la aplica `target_for*`; qué
    /// modelos están habilitados lo declara el estado del FSM. Este bucle sólo
    /// ejecuta y cuenta.
    fn run_child_models(
        &mut self,
        ordered: &[&str],
        fb: &FrameBuffer,
        pending: &mut Vec<PendingModelOutput>,
        now: Instant,
        urgent_request: Option<&InferenceRequest>,
    ) {
        let children: Vec<String> = ordered
            .iter()
            .filter(|model| self.cascade.parent_of(model).is_some())
            .map(|model| (*model).to_owned())
            .collect();
        for model_key in children {
            let urgent = urgent_request.filter(|request| request.model_key == model_key);
            if urgent.is_none() && !self.cascade.is_due(&model_key, now) {
                let interval = self.cascade.interval_min_ms(&model_key).unwrap_or(0);
                let mut metrics = self.lock_metrics();
                metrics.set_model_interval(&model_key, interval);
                metrics.tick_infer_not_due(&model_key);
                continue;
            }
            let target = self.resolve_cascade_target(&model_key, pending, fb);
            if target.is_none() {
                let interval = self.cascade.interval_min_ms(&model_key).unwrap_or(0);
                let mut metrics = self.lock_metrics();
                metrics.set_model_interval(&model_key, interval);
                metrics.tick_infer_skip(&model_key);
                metrics.tick_infer_due_but_no_target(&model_key);
                continue;
            }
            self.run_scheduled_model(&model_key, target, fb, pending, now, urgent);
        }
    }

    /// Modelos que el catálogo habilita y **el estado del FSM no pidió**.
    ///
    /// Es una razón distinta de `skips` y hay que poder distinguirlas: un hijo
    /// salteado por su regla vio la escena y no aplicó; un hijo apagado por el
    /// estado nunca llegó a mirarla. Sin este contador, mover una política al
    /// catálogo hace que un modelo deje de correr **en silencio**, y el
    /// silencio es lo que dejó a la cascada muerta sin que nadie se enterara.
    fn count_models_gated_by_state(
        &mut self,
        ordered: &[&str],
        config: &PerceptionConfig,
        now: Instant,
    ) {
        let gated: Vec<String> = self
            .cascade
            .all_models()
            .iter()
            .filter(|name| self.is_model_enabled(config, name))
            .filter(|name| !ordered.contains(&name.as_str()))
            .cloned()
            .collect();
        if gated.is_empty() {
            return;
        }
        let mut metrics = self.lock_metrics();
        for name in gated {
            let interval = self.cascade.interval_min_ms(&name).unwrap_or(0);
            let due = self.cascade.is_due(&name, now);
            metrics.set_model_interval(&name, interval);
            metrics.tick_infer_gated(&name);
            if due {
                metrics.tick_infer_due_but_gated(&name);
            }
        }
    }

    fn resolve_cascade_target(
        &self,
        model_key: &str,
        pending: &[PendingModelOutput],
        fb: &FrameBuffer,
    ) -> Option<CascadeTarget> {
        if self.cascade.same_frame(model_key) {
            self.cascade
                .parent_of(model_key)
                .and_then(|parent| {
                    pending
                        .iter()
                        .find(|item| item.model_key == parent)
                        .map(|item| item.output.detections.as_slice())
                })
                .and_then(|detections| {
                    self.cascade
                        .target_for_detections(model_key, detections, fb.w, fb.h)
                })
        } else {
            let observations = gate_observations(&self.directive.tracks);
            self.cascade
                .target_for(model_key, &observations, fb.w, fb.h)
        }
    }

    /// Stage: consolidate non-depth outputs and emit per-observation events.
    fn consolidate_and_emit(
        &mut self,
        pending: &[PendingModelOutput],
        frame_number: u64,
    ) -> Vec<ConsolidatedObservation> {
        let mut model_outputs: Vec<ModelDetections> = Vec::new();
        model_outputs.extend(
            pending
                .iter()
                .filter(|item| !self.models.is_depth(&item.model_key))
                .map(|item| ModelDetections {
                    model: item.model_key.as_str(),
                    role: if item.model_key == self.primary_model {
                        DetectionRole::Primary
                    } else {
                        DetectionRole::Secondary
                    },
                    detections: &item.output.detections,
                }),
        );
        let observations = self.detection_consolidator.consolidate(&model_outputs);
        for observation in &observations {
            let mut sources: Vec<String> = observation
                .evidence
                .iter()
                .chain(observation.components.iter())
                .map(|e| e.model.clone())
                .collect();
            sources.sort();
            sources.dedup();
            self.observer.emit(Event::consolidated_detection(
                frame_number,
                &observation.class,
                observation.confidence,
                observation.bbox,
                &observation.primary_model,
                sources,
            ));
        }
        observations
    }

    /// Stage: record each pending model result (metrics, viz, JSONL, depth).
    fn record_pending_results(
        &mut self,
        pending: Vec<PendingModelOutput>,
        frame: FrameSize,
        now: Instant,
    ) {
        for item in pending {
            self.record_model_result(
                &item.model_key,
                &item.output,
                item.crop_frame,
                item.crop_rect,
                frame,
                now,
            );
        }
    }

    fn validate_cross_model_from_pending(
        &self,
        pending: &[PendingModelOutput],
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
        validation_config: &crate::config::CrossModelValidationConfig,
        face_pose_config: &crate::config::FacePoseConfig,
    ) -> Vec<CrossModelValidation> {
        let inputs = pending
            .iter()
            .filter_map(|item| {
                let kind = if self.models.is_face_model(&item.model_key) {
                    EvidenceKind::Face
                } else if self.models.is_pose(&item.model_key) {
                    EvidenceKind::Pose
                } else if self.models.is_segment(&item.model_key) {
                    EvidenceKind::Segment
                } else if self.models.is_box_model(&item.model_key) {
                    EvidenceKind::Detection
                } else {
                    return None;
                };
                Some(PendingEvidence {
                    model_key: item.model_key.as_str(),
                    kind,
                    target: item.target?,
                    detections: &item.output.detections,
                })
            })
            .collect::<Vec<_>>();
        validate_pending(
            &inputs,
            frame_number,
            frame_width,
            frame_height,
            validation_config,
            face_pose_config,
        )
    }

    fn estimate_body_parts_from_pending(
        &mut self,
        pending: &[PendingModelOutput],
        validations: &[CrossModelValidation],
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
        config: &crate::config::BodyPartsConfig,
    ) -> Vec<BodyPartsEstimate> {
        let inputs = pending
            .iter()
            .filter_map(|item| {
                let kind = if self.models.is_face_model(&item.model_key) {
                    EvidenceKind::Face
                } else if self.models.is_pose(&item.model_key) {
                    EvidenceKind::Pose
                } else if self.models.is_segment(&item.model_key) {
                    EvidenceKind::Segment
                } else if self.models.is_box_model(&item.model_key) {
                    EvidenceKind::Detection
                } else {
                    return None;
                };
                Some(PendingBodyPartsEvidence {
                    model_key: item.model_key.as_str(),
                    kind,
                    target: item.target,
                    detections: &item.output.detections,
                })
            })
            .collect::<Vec<_>>();
        let estimator = BodyPartsEstimator::new(config);
        if matches!(config.mode, crate::config::BodyPartsMode::Advanced) {
            estimator.estimate_advanced(
                &inputs,
                validations,
                frame_number,
                frame_width,
                frame_height,
                &mut self.body_parts_temporal,
            )
        } else {
            estimator.estimate(
                &inputs,
                validations,
                frame_number,
                frame_width,
                frame_height,
            )
        }
    }

    fn attach_body_parts_depth(
        &self,
        estimates: &mut [BodyPartsEstimate],
        pending: &[PendingModelOutput],
        frame_width: u32,
        frame_height: u32,
    ) {
        for estimate in estimates {
            let Some(depth_output) = self.depth_output_for_actor(&estimate.actor_ref, pending)
            else {
                continue;
            };
            let Some(depth) = depth_output.output.depth.as_ref() else {
                continue;
            };
            let Some(roi) = depth_output
                .crop_rect
                .or(self.depth_context_roi)
                .map(CropRect::to_array)
            else {
                continue;
            };
            attach_depth(
                estimate,
                &depth_output.model_key,
                depth,
                roi,
                frame_width,
                frame_height,
            );
        }
    }

    fn depth_output_for_actor<'a>(
        &self,
        actor_ref: &ActorRef,
        pending: &'a [PendingModelOutput],
    ) -> Option<&'a PendingModelOutput> {
        let matches_actor = |item: &&PendingModelOutput| {
            self.models.is_depth(&item.model_key)
                && item.output.depth.is_some()
                && item.crop_rect.is_some()
                && match actor_ref {
                    ActorRef::Track(actor_id) => item
                        .target
                        .is_some_and(|target| target.id == Some(*actor_id)),
                    ActorRef::FrameLocal { .. } => item.target.is_none(),
                }
        };
        pending.iter().filter(matches_actor).next().or_else(|| {
            pending.iter().find(|item| {
                self.models.is_depth(&item.model_key)
                    && item.output.depth.is_some()
                    && item.crop_rect.is_some()
                    && item.target.is_none()
            })
        })
    }

    fn attach_body_parts_surface_evidence(
        &self,
        estimates: &mut [BodyPartsEstimate],
        pending: &[PendingModelOutput],
        frame_width: u32,
        frame_height: u32,
    ) {
        let Some(calibration) = self.surface_calibration.as_ref() else {
            return;
        };
        let Some(scene_output) = pending.iter().find(|item| {
            self.models.is_depth(&item.model_key)
                && item.output.depth.is_some()
                && item.crop_rect.is_some()
                && item.crop_rect == self.depth_context_roi
        }) else {
            return;
        };
        let Some(depth) = scene_output.output.depth.as_ref() else {
            return;
        };
        let Some(roi) = scene_output
            .crop_rect
            .or(self.depth_context_roi)
            .map(CropRect::to_array)
        else {
            return;
        };
        attach_surface_evidence(
            estimates,
            &scene_output.model_key,
            depth,
            roi,
            frame_width,
            frame_height,
            calibration,
        );
    }

    /// Stage: project consolidated sample into the control process image.
    fn publish_clinical_sample(&mut self, sample: ClinicalSample, now: Instant) {
        self.image.observations = Some(mana_control::AgedEvidence::new(
            project_scene_sample(&sample),
            now,
        ));
        self.image.measurement_pending = true;
    }

    fn run_scheduled_model(
        &mut self,
        model_key: &str,
        target: Option<CascadeTarget>,
        fb: &FrameBuffer,
        pending: &mut Vec<PendingModelOutput>,
        now: Instant,
        urgent_request: Option<&InferenceRequest>,
    ) -> bool {
        let is_static = self
            .infer
            .crop_info(model_key)
            .is_some_and(|c| c.crop_type == CropType::Static);
        let crop_rect = self.resolve_crop_rect(model_key, target, fb);
        let manual_crop = if is_static { None } else { crop_rect };
        // Mark and consume before entering the synchronous backend so a failed
        // model cannot be retried on every incoming keyframe and a failed
        // urgent attempt cannot be replayed.
        let Some(timing) = self.cascade.mark_started(model_key, now) else {
            return false;
        };
        if let Some(request) = urgent_request {
            if !self.cascade.consume_request(request) {
                return false;
            }
            let wait = now.saturating_duration_since(request.requested_at);
            let mut metrics = self.lock_metrics();
            metrics.tick_infer_urgent(model_key);
            metrics.tick_urgent_wait(model_key, wait);
            metrics.tick_inference_start(model_key, timing);
        } else {
            self.lock_metrics().tick_inference_start(model_key, timing);
        }
        let Some(mut output) = self.infer.run(model_key, &fb.rgb, fb.w, fb.h, manual_crop) else {
            return false;
        };
        let crop_frame = output.crop_frame.take();
        pending.push(PendingModelOutput {
            model_key: model_key.to_owned(),
            output,
            target,
            crop_frame,
            crop_rect,
        });
        true
    }

    fn expire_face_pose_context(&mut self, now: Instant) {
        if self
            .face_pose_context
            .is_some_and(|context| context.expires_at <= now)
        {
            self.face_pose_context = None;
        }
    }

    fn validate_face_pose_from_pending(
        &mut self,
        pending: &[PendingModelOutput],
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
        config: &PerceptionConfig,
    ) -> Option<mana_control::FacePoseValidation> {
        let context = self.face_pose_context?;
        let pose_output = pending.iter().find(|item| {
            item.model_key == config.face_pose.pose_model_key
                && self.models.is_pose(&item.model_key)
        });
        let Some(pose_output) = pose_output else {
            return None;
        };
        let Some(target) = pose_output.target else {
            self.face_pose_context = None;
            return None;
        };
        let Some(pose) = select_pose_detection(&pose_output.output.detections, target) else {
            self.face_pose_context = None;
            return None;
        };
        self.face_pose_context = None;
        validate_face_pose(
            context,
            Some(target),
            pose,
            frame_number,
            frame_width,
            frame_height,
            &config.face_pose,
        )
    }

    fn request_face_pose_from_pending(
        &mut self,
        pending: &[PendingModelOutput],
        frame_number: u64,
        config: &PerceptionConfig,
        now: Instant,
    ) {
        if !config.face_pose.enabled || self.face_pose_context.is_some() {
            return;
        }
        let pose_model = config.face_pose.pose_model_key.as_str();
        if !self
            .cascade
            .all_models()
            .iter()
            .any(|model| model == pose_model)
            || !self.models.enabled(pose_model)
            || !self.infer.has_model(pose_model)
        {
            return;
        }

        let candidate = pending
            .iter()
            .filter(|item| self.models.is_face_model(&item.model_key))
            .filter_map(|item| {
                let target = item.target?;
                let face = item
                    .output
                    .detections
                    .iter()
                    .filter(|detection| detection.class == "face")
                    .filter(|detection| is_uncertain_face(detection, &config.face_pose))
                    .max_by(|left, right| left.confidence.total_cmp(&right.confidence))?;
                Some((face.bbox, face.confidence, target))
            })
            .max_by(|(_, left_confidence, _), (_, right_confidence, _)| {
                left_confidence.total_cmp(right_confidence)
            });
        let Some((face_bbox, face_confidence, target)) = candidate else {
            return;
        };
        if target.id.is_none() {
            return;
        }

        let context = PendingFacePoseContext {
            face_bbox,
            face_confidence,
            target,
            source_frame_number: frame_number,
            requested_at: now,
            expires_at: now + Duration::from_millis(config.face_pose.request_ttl_ms),
        };
        let request = context.request(&config.face_pose);
        if self.enqueue_transient_request(request, config, now) {
            self.face_pose_context = Some(context);
        }
    }

    fn resolve_crop_rect(
        &self,
        model_key: &str,
        target: Option<CascadeTarget>,
        fb: &FrameBuffer,
    ) -> Option<crate::detection::CropRect> {
        let crop_cfg = self.infer.crop_info(model_key)?;

        if crop_cfg.crop_type == CropType::Static {
            return crop_cfg.region.map(CropRect::from_array);
        }

        let target = target?;
        if let Some(square_size) = crop_cfg.square_size {
            return compute_upper_square_roi(
                target.bbox,
                square_size,
                crop_cfg.upper_fraction.unwrap_or(0.5),
                fb.w,
                fb.h,
            );
        }
        compute_bbox_roi(
            target.bbox,
            crop_cfg.margin,
            fb.w,
            fb.h,
            crop_cfg.min_region,
            crop_cfg.max_region,
        )
    }

    fn is_model_enabled(&self, config: &PerceptionConfig, name: &str) -> bool {
        self.models.enabled(name)
            && self
                .models
                .task_of(name)
                .map(|task| {
                    !config
                        .disabled_tasks
                        .iter()
                        .any(|disabled| disabled == task.as_str())
                })
                .unwrap_or(true)
    }

    /// Qué modelos correr. El otro brazo de la realimentación: **el FSM decide
    /// qué mira la percepción**, y el FSM vive del lado de control.
    ///
    /// Sin directiva todavía —los primeros keyframes antes del primer scan— se
    /// cae a la cascada completa, que es lo que hacía el sistema cuando el FSM
    /// estaba apagado. Arrancar sin modelos sería peor: no habría evidencia con
    /// la que control pudiera producir la primera directiva.
    fn resolve_models(&self, config: &PerceptionConfig) -> Vec<String> {
        let models = if self.directive.models.is_empty() {
            self.cascade.all_models().to_vec()
        } else {
            self.directive.models.clone()
        };
        models
            .into_iter()
            .filter(|name| self.is_model_enabled(config, name))
            .collect()
    }
}

fn observations_person_count(observations: &[ConsolidatedObservation], class: &str) -> usize {
    observations
        .iter()
        .filter(|observation| observation.class == class)
        .count()
}

fn body_parts_event(estimate: &BodyPartsEstimate) -> Event {
    let (actor_id, frame_local_index) = match estimate.actor_ref {
        ActorRef::Track(actor_id) => (Some(actor_id), None),
        ActorRef::FrameLocal { index, .. } => (None, Some(index)),
    };
    Event::body_parts(
        estimate.frame_number,
        actor_id,
        frame_local_index,
        estimate.overall_quality,
        estimate
            .parts
            .iter()
            .map(|part| crate::logger::BodyPartRecord {
                part: part.part.as_str().into(),
                geometry: match &part.geometry {
                    BodyGeometry::Bbox(bbox) => crate::logger::BodyGeometryRecord::Bbox(*bbox),
                    BodyGeometry::Polygon(points) => {
                        crate::logger::BodyGeometryRecord::Polygon(points.clone())
                    }
                    BodyGeometry::Polyline { points, radius } => {
                        crate::logger::BodyGeometryRecord::Polyline {
                            points: points.clone(),
                            radius: *radius,
                        }
                    }
                },
                support: part
                    .support
                    .iter()
                    .map(|support| support.as_str().into())
                    .collect(),
                source_models: part.source_models.clone(),
                quality: part.quality,
                mask_coverage: part.mask_coverage,
                depth: part
                    .depth
                    .as_ref()
                    .map(|depth| crate::logger::BodyPartDepthRecord {
                        source_model: depth.source_model.clone(),
                        roi: depth.roi,
                        map_width: depth.map_width,
                        map_height: depth.map_height,
                        sampled_pixels: depth.sampled_pixels,
                        valid_pixels: depth.valid_pixels,
                        valid_ratio: depth.valid_ratio,
                        min_depth_m: depth.min_depth_m,
                        median_depth_m: depth.median_depth_m,
                        p10_depth_m: depth.p10_depth_m,
                        p90_depth_m: depth.p90_depth_m,
                        max_depth_m: depth.max_depth_m,
                        relative_to_torso_m: depth.relative_to_torso_m,
                        surface_evidence: depth
                            .surface_evidence
                            .iter()
                            .map(|evidence| crate::logger::SurfaceEvidenceRecord {
                                source_model: evidence.source_model.clone(),
                                surface: evidence.surface.clone(),
                                zone: evidence.zone.clone(),
                                sampled_pixels: evidence.sampled_pixels,
                                valid_ratio: evidence.valid_ratio,
                                observed_median: evidence.observed_median,
                                reference_median: evidence.reference_median,
                                residual: evidence.residual,
                                in_envelope: evidence.in_envelope,
                            })
                            .collect(),
                    }),
                source_frame_numbers: part.source_frame_numbers.clone(),
                stale: part.stale,
            })
            .collect(),
    )
}

fn gate_observations(tracks: &[mana_control::track::Track]) -> Vec<GateObservation> {
    tracks
        .iter()
        .map(|t| GateObservation {
            id: t.id,
            bbox: t.bbox,
            class: ClassName::new(t.class.as_str()),
            confidence: t.confidence,
            source_model: ModelId::new(t.source_model.as_str()),
            is_confirmed: t.is_confirmed,
            misses: t.misses,
        })
        .collect()
}

/// Application adapter from perception's rich consolidated evidence to the
/// narrow control input port. Mask payloads and model-specific components stay
/// on the perception side; control receives only scene facts it can decide on.
fn project_scene_sample(sample: &ClinicalSample) -> mana_control::SceneSample {
    mana_control::SceneSample {
        observations: sample
            .observations
            .iter()
            .map(|observation| {
                let mut source_models: Vec<_> = observation
                    .evidence
                    .iter()
                    .chain(&observation.components)
                    .map(|evidence| mana_control::domain::ModelId::new(evidence.model.as_str()))
                    .collect();
                source_models.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                source_models.dedup();
                let face = observation
                    .components
                    .iter()
                    .filter(|component| component.class == "face")
                    .max_by(|a, b| a.confidence.total_cmp(&b.confidence))
                    .map(|face| mana_control::FaceObservation {
                        bbox: face.bbox,
                        confidence: face.confidence,
                    });
                mana_control::SceneObservation {
                    class: mana_control::domain::ClassName::new(observation.class.as_str()),
                    bbox: observation.bbox,
                    confidence: observation.confidence,
                    source_models,
                    face,
                }
            })
            .collect(),
        signal_valid: sample.signal_valid,
        raw_person_count: sample.raw_person_count,
        frame_number: sample.frame_number,
        face_model_ran: sample.face_model_ran,
        face_pose_validation: sample.face_pose_validation,
    }
}
