//! Stateless, explainable body-part geometry kept on the perception side.
//!
//! The estimator consumes model outputs that already ran in the current
//! keyframe. It derives local geometry from pose/face/segment evidence, but it
//! never schedules a model and it never crosses raw evidence into control.

use std::collections::BTreeMap;

use super::cross_model_validation::{CrossModelValidation, EvidenceKind};
use crate::cascade::CascadeTarget;
use crate::config::BodyPartsConfig;
use crate::depth_map::DepthFrame;
use crate::detection::Detection;
use mana_perception::{SurfaceCalibration, SurfaceLayer, polygon_stats_intersecting_clips};

/// Temporal identity for a derived estimate. `FrameLocal` is deliberately not
/// persisted: it is only an honest label for same-frame, untracked evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ActorRef {
    Track(u64),
    FrameLocal { frame_number: u64, index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BodyPartKind {
    Head,
    Torso,
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}

impl BodyPartKind {
    pub(crate) const ALL: [Self; 6] = [
        Self::Head,
        Self::Torso,
        Self::LeftArm,
        Self::RightArm,
        Self::LeftLeg,
        Self::RightLeg,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::Torso => "torso",
            Self::LeftArm => "left_arm",
            Self::RightArm => "right_arm",
            Self::LeftLeg => "left_leg",
            Self::RightLeg => "right_leg",
        }
    }
}

const HEAD_JOINTS: [usize; 5] = [0, 1, 2, 3, 4];
const TORSO_JOINTS: [usize; 4] = [5, 6, 11, 12];
const LIMB_JOINTS: [(BodyPartKind, [usize; 3]); 4] = [
    (BodyPartKind::LeftArm, [5, 7, 9]),
    (BodyPartKind::RightArm, [6, 8, 10]),
    (BodyPartKind::LeftLeg, [11, 13, 15]),
    (BodyPartKind::RightLeg, [12, 14, 16]),
];

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BodyPartSupport {
    Face,
    Pose,
    Segment,
    Temporal,
}

impl BodyPartSupport {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Face => "face",
            Self::Pose => "pose",
            Self::Segment => "segment",
            Self::Temporal => "temporal",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BodyGeometry {
    Bbox([f32; 4]),
    Polygon(Vec<[f32; 2]>),
    Polyline { points: Vec<[f32; 2]>, radius: f32 },
}

/// Robust depth evidence attached to one derived body part.
///
/// The value is deliberately evidence, not a posture decision. `roi` and the
/// map dimensions identify the depth source used for the sample, while
/// `relative_to_torso_m` is only meaningful when both parts came from the same
/// depth map.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BodyPartDepth {
    pub(crate) source_model: String,
    pub(crate) roi: [u32; 4],
    pub(crate) map_width: u32,
    pub(crate) map_height: u32,
    pub(crate) sampled_pixels: u64,
    pub(crate) valid_pixels: u64,
    pub(crate) valid_ratio: Option<f32>,
    pub(crate) min_depth_m: Option<f32>,
    pub(crate) median_depth_m: Option<f32>,
    pub(crate) p10_depth_m: Option<f32>,
    pub(crate) p90_depth_m: Option<f32>,
    pub(crate) max_depth_m: Option<f32>,
    pub(crate) relative_to_torso_m: Option<f32>,
    pub(crate) surface_evidence: Vec<SurfaceDepthEvidence>,
}

/// Depth evidence relative to one calibrated scene surface zone.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceDepthEvidence {
    pub(crate) source_model: String,
    pub(crate) surface: String,
    pub(crate) zone: String,
    pub(crate) sampled_pixels: u64,
    pub(crate) valid_ratio: Option<f32>,
    pub(crate) observed_median: Option<f32>,
    pub(crate) reference_median: f32,
    pub(crate) residual: Option<f32>,
    pub(crate) in_envelope: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BodyPartEstimate {
    pub(crate) part: BodyPartKind,
    pub(crate) geometry: BodyGeometry,
    pub(crate) support: Vec<BodyPartSupport>,
    pub(crate) source_models: Vec<String>,
    pub(crate) quality: f32,
    pub(crate) mask_coverage: Option<f32>,
    pub(crate) depth: Option<BodyPartDepth>,
    pub(crate) source_frame_numbers: Vec<u64>,
    pub(crate) stale: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BodyPartsEstimate {
    pub(crate) actor_ref: ActorRef,
    pub(crate) frame_number: u64,
    pub(crate) parts: Vec<BodyPartEstimate>,
    pub(crate) overall_quality: f32,
    /// Normalized full-frame contours retained for depth sampling. Keeping the
    /// mask once per actor avoids copying the same segmentation into every
    /// body-part record.
    pub(crate) mask_polygons: Option<Vec<Vec<[f32; 2]>>>,
}

#[derive(Debug, Clone)]
struct HistoricalPart {
    geometry: BodyGeometry,
    actor_bbox: [f32; 4],
    quality: f32,
    source_models: Vec<String>,
    source_frame_numbers: Vec<u64>,
    observed_frame: u64,
}

#[derive(Debug, Clone, Default)]
struct HistoricalActor {
    last_seen_frame: u64,
    parts: BTreeMap<BodyPartKind, HistoricalPart>,
}

/// Temporal memory used only by the opt-in advanced estimator.
///
/// The validator remains stateless. This state is keyed by the tracker id and
/// is never used for frame-local actors, so it cannot turn an untracked spatial
/// match into a false temporal identity.
#[derive(Debug, Default)]
pub(crate) struct BodyPartsTemporalState {
    actors: BTreeMap<u64, HistoricalActor>,
}

impl BodyPartsTemporalState {
    fn prune(&mut self, frame_number: u64, max_gap_frames: u64) {
        for actor in self.actors.values_mut() {
            actor.parts.retain(|_, part| {
                frame_number.saturating_sub(part.observed_frame) <= max_gap_frames
            });
        }
        self.actors.retain(|_, actor| {
            frame_number.saturating_sub(actor.last_seen_frame) <= max_gap_frames
                && !actor.parts.is_empty()
        });
    }
}

/// Rich model output adapted at the inference boundary. The raw `Detection`
/// type stays unchanged and remains local to perception.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingBodyPartsEvidence<'a> {
    pub(crate) model_key: &'a str,
    pub(crate) kind: EvidenceKind,
    pub(crate) target: Option<CascadeTarget>,
    pub(crate) detections: &'a [Detection],
}

#[derive(Debug, Clone, Copy)]
struct SourceDetection<'a> {
    model_key: &'a str,
    detection: &'a Detection,
}

#[derive(Debug)]
struct ActorEvidence<'a> {
    actor_ref: ActorRef,
    target_bbox: [f32; 4],
    face: Option<SourceDetection<'a>>,
    pose: Option<SourceDetection<'a>>,
    segment: Option<SourceDetection<'a>>,
}

impl<'a> ActorEvidence<'a> {
    fn new(actor_ref: ActorRef, target_bbox: [f32; 4]) -> Self {
        Self {
            actor_ref,
            target_bbox,
            face: None,
            pose: None,
            segment: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Joint {
    point: [f32; 2],
    confidence: f32,
}

/// Stateless body-part estimator for the Sprint 2 MVP.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BodyPartsEstimator<'a> {
    config: &'a BodyPartsConfig,
}

impl<'a> BodyPartsEstimator<'a> {
    pub(crate) const fn new(config: &'a BodyPartsConfig) -> Self {
        Self { config }
    }

    /// Estimates all actors represented by the current keyframe's evidence.
    ///
    /// Tracked targets are associated only by their explicit id. Untracked
    /// inputs are emitted as `FrameLocal` and may be associated spatially only
    /// inside this keyframe; they have no temporal identity.
    #[must_use]
    pub(crate) fn estimate(
        &self,
        inputs: &[PendingBodyPartsEvidence<'_>],
        validations: &[CrossModelValidation],
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
    ) -> Vec<BodyPartsEstimate> {
        if frame_width == 0 || frame_height == 0 {
            return Vec::new();
        }

        let actors = self.collect_actors(inputs, frame_number);
        actors
            .into_iter()
            .filter_map(|actor| {
                let validation_quality = match actor.actor_ref {
                    ActorRef::Track(actor_id) => validations
                        .iter()
                        .find(|validation| validation.actor_id == actor_id)
                        .map(|validation| validation.quality),
                    ActorRef::FrameLocal { .. } => None,
                };
                self.estimate_actor(
                    actor,
                    validation_quality,
                    frame_number,
                    frame_width,
                    frame_height,
                )
            })
            .collect()
    }

    /// Estimates body parts with conservative temporal completion.
    ///
    /// A previous geometry is reused only when the current instance mask
    /// supports it. Without that evidence the method returns the same partial
    /// geometry as the validator instead of fabricating a limb.
    #[must_use]
    pub(crate) fn estimate_advanced(
        &self,
        inputs: &[PendingBodyPartsEvidence<'_>],
        validations: &[CrossModelValidation],
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
        temporal: &mut BodyPartsTemporalState,
    ) -> Vec<BodyPartsEstimate> {
        if frame_width == 0 || frame_height == 0 {
            return Vec::new();
        }

        temporal.prune(frame_number, self.config.advanced_max_gap_frames);
        let actors = self.collect_actors(inputs, frame_number);
        actors
            .into_iter()
            .filter_map(|actor| {
                let validation_quality = match actor.actor_ref {
                    ActorRef::Track(actor_id) => validations
                        .iter()
                        .find(|validation| validation.actor_id == actor_id)
                        .map(|validation| validation.quality),
                    ActorRef::FrameLocal { .. } => None,
                };
                let actor_ref = actor.actor_ref.clone();
                let actor_bbox = actor.target_bbox;
                let segment = actor.segment;
                let current = self.estimate_actor(
                    actor,
                    validation_quality,
                    frame_number,
                    frame_width,
                    frame_height,
                );
                match actor_ref {
                    ActorRef::Track(actor_id) => self.enhance_tracked_actor(
                        current,
                        actor_id,
                        actor_bbox,
                        segment,
                        validation_quality,
                        frame_number,
                        frame_width,
                        frame_height,
                        temporal,
                    ),
                    ActorRef::FrameLocal { .. } => current,
                }
            })
            .collect()
    }

    fn enhance_tracked_actor(
        &self,
        current: Option<BodyPartsEstimate>,
        actor_id: u64,
        actor_bbox: [f32; 4],
        segment: Option<SourceDetection<'_>>,
        validation_quality: Option<f32>,
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
        temporal: &mut BodyPartsTemporalState,
    ) -> Option<BodyPartsEstimate> {
        let previous = temporal.actors.get(&actor_id).cloned();
        let current_parts = current
            .as_ref()
            .map(|estimate| estimate.parts.clone())
            .unwrap_or_default();
        let current_by_kind = current_parts
            .iter()
            .map(|part| (part.part, part))
            .collect::<BTreeMap<_, _>>();
        let mut parts = Vec::with_capacity(BodyPartKind::ALL.len());

        for current_part in current_parts.iter().cloned() {
            let previous_part = previous
                .as_ref()
                .and_then(|actor| actor.parts.get(&current_part.part));
            parts.push(self.stabilize_current_part(
                current_part,
                previous_part,
                actor_bbox,
                segment,
                validation_quality,
                frame_number,
                frame_width,
                frame_height,
            ));
        }

        if let Some(previous_actor) = &previous {
            for (part_kind, previous_part) in &previous_actor.parts {
                if current_by_kind.contains_key(part_kind) {
                    continue;
                }
                let gap = frame_number.saturating_sub(previous_part.observed_frame);
                if gap > self.config.advanced_max_gap_frames {
                    continue;
                }
                let predicted = transform_geometry(
                    &previous_part.geometry,
                    previous_part.actor_bbox,
                    actor_bbox,
                );
                let coverage =
                    self.predicted_mask_coverage(&predicted, segment, frame_width, frame_height);
                if !mask_supports_prediction(coverage, self.config.advanced_mask_support_threshold)
                {
                    continue;
                }
                parts.push(self.temporal_fallback_part(
                    *part_kind,
                    previous_part,
                    predicted,
                    coverage,
                    segment,
                    validation_quality,
                    frame_number,
                ));
            }
        }

        if parts.is_empty() {
            return None;
        }
        parts.sort_by_key(|part| part.part);

        let mask_polygons = current
            .as_ref()
            .and_then(|estimate| estimate.mask_polygons.clone())
            .or_else(|| {
                segment.and_then(|source| {
                    source
                        .detection
                        .mask
                        .as_ref()
                        .map(|mask| mask.polygons.as_ref().clone())
                })
            });

        self.remember_current_parts(
            temporal,
            actor_id,
            actor_bbox,
            frame_number,
            &current_parts,
            segment,
        );

        Some(BodyPartsEstimate {
            actor_ref: ActorRef::Track(actor_id),
            frame_number,
            overall_quality: overall_quality(&parts),
            parts,
            mask_polygons,
        })
    }

    fn stabilize_current_part(
        &self,
        mut current: BodyPartEstimate,
        previous: Option<&HistoricalPart>,
        actor_bbox: [f32; 4],
        segment: Option<SourceDetection<'_>>,
        validation_quality: Option<f32>,
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
    ) -> BodyPartEstimate {
        let Some(previous) = previous else {
            return current;
        };
        let gap = frame_number.saturating_sub(previous.observed_frame);
        if gap > self.config.advanced_max_gap_frames {
            return current;
        }
        let predicted = transform_geometry(&previous.geometry, previous.actor_bbox, actor_bbox);
        let predicted_coverage =
            self.predicted_mask_coverage(&predicted, segment, frame_width, frame_height);
        let current_is_complete = complete_geometry(current.part, &current.geometry);
        let current_mask_rejects = current
            .mask_coverage
            .is_some_and(|coverage| coverage < self.config.advanced_mask_support_threshold);

        if (!current_is_complete || current_mask_rejects)
            && mask_supports_prediction(
                predicted_coverage,
                self.config.advanced_mask_support_threshold,
            )
        {
            current.geometry = predicted;
            current.quality = self.temporal_quality(
                previous.quality,
                gap,
                predicted_coverage,
                segment,
                validation_quality,
            );
            current.mask_coverage = predicted_coverage.or(current.mask_coverage);
            current.stale = true;
            add_support(&mut current.support, BodyPartSupport::Temporal);
            merge_source_models(&mut current.source_models, &previous.source_models);
            merge_frame_numbers(
                &mut current.source_frame_numbers,
                &previous.source_frame_numbers,
                frame_number,
            );
            return current;
        }

        if current_is_complete
            && !current_mask_rejects
            && (segment.is_none()
                || current.mask_coverage.is_some_and(|coverage| {
                    coverage >= self.config.advanced_mask_support_threshold
                }))
        {
            let smoothed = blend_geometry(
                &current.geometry,
                &predicted,
                self.config.advanced_smoothing_alpha,
            );
            if smoothed != current.geometry {
                current.geometry = smoothed;
                current.quality = weighted_average(
                    current.quality,
                    previous.quality,
                    self.config.advanced_smoothing_alpha,
                    1.0 - self.config.advanced_smoothing_alpha,
                );
                current.stale = false;
                add_support(&mut current.support, BodyPartSupport::Temporal);
                merge_source_models(&mut current.source_models, &previous.source_models);
                merge_frame_numbers(
                    &mut current.source_frame_numbers,
                    &previous.source_frame_numbers,
                    frame_number,
                );
            }
        }
        current
    }

    fn temporal_fallback_part(
        &self,
        part: BodyPartKind,
        previous: &HistoricalPart,
        geometry: BodyGeometry,
        mask_coverage: Option<f32>,
        segment: Option<SourceDetection<'_>>,
        validation_quality: Option<f32>,
        frame_number: u64,
    ) -> BodyPartEstimate {
        let gap = frame_number.saturating_sub(previous.observed_frame);
        let mut source_models = previous.source_models.clone();
        if let Some(segment) = segment {
            source_models.push(segment.model_key.to_owned());
        }
        source_models.sort_unstable();
        source_models.dedup();
        let mut source_frame_numbers = previous.source_frame_numbers.clone();
        source_frame_numbers.push(frame_number);
        source_frame_numbers.sort_unstable();
        source_frame_numbers.dedup();
        let quality = self.temporal_quality(
            previous.quality,
            gap,
            mask_coverage,
            segment,
            validation_quality,
        );
        BodyPartEstimate {
            part,
            geometry,
            support: vec![BodyPartSupport::Segment, BodyPartSupport::Temporal],
            source_models,
            quality,
            mask_coverage,
            depth: None,
            source_frame_numbers,
            stale: true,
        }
    }

    fn temporal_quality(
        &self,
        previous_quality: f32,
        gap: u64,
        mask_coverage: Option<f32>,
        segment: Option<SourceDetection<'_>>,
        validation_quality: Option<f32>,
    ) -> f32 {
        let decay = self
            .config
            .advanced_temporal_quality_decay
            .powi(gap.min(i32::MAX as u64) as i32);
        let mut quality = clamp01(previous_quality * decay);
        if let (Some(coverage), Some(segment)) = (mask_coverage, segment) {
            quality = weighted_average(
                quality,
                coverage * clamp01(segment.detection.confidence),
                1.0 - self.config.mask_quality_weight,
                self.config.mask_quality_weight,
            );
        }
        apply_validation_quality(&mut quality, validation_quality, self.config);
        quality
    }

    fn predicted_mask_coverage(
        &self,
        geometry: &BodyGeometry,
        segment: Option<SourceDetection<'_>>,
        frame_width: u32,
        frame_height: u32,
    ) -> Option<f32> {
        segment.and_then(|source| {
            source.detection.mask.as_ref().and_then(|mask| {
                mask_coverage(
                    geometry,
                    mask,
                    frame_width,
                    frame_height,
                    self.config.geometry_epsilon,
                )
            })
        })
    }

    fn remember_current_parts(
        &self,
        temporal: &mut BodyPartsTemporalState,
        actor_id: u64,
        actor_bbox: [f32; 4],
        frame_number: u64,
        parts: &[BodyPartEstimate],
        segment: Option<SourceDetection<'_>>,
    ) {
        let actor = temporal.actors.entry(actor_id).or_default();
        actor.last_seen_frame = frame_number;
        for part in parts {
            if !complete_geometry(part.part, &part.geometry)
                || (segment.is_some()
                    && !part.mask_coverage.is_some_and(|coverage| {
                        coverage >= self.config.advanced_mask_support_threshold
                    }))
            {
                continue;
            }
            actor.parts.insert(
                part.part,
                HistoricalPart {
                    geometry: part.geometry.clone(),
                    actor_bbox,
                    quality: part.quality,
                    source_models: part.source_models.clone(),
                    source_frame_numbers: part.source_frame_numbers.clone(),
                    observed_frame: frame_number,
                },
            );
        }
    }

    fn collect_actors<'e>(
        &self,
        inputs: &[PendingBodyPartsEvidence<'e>],
        frame_number: u64,
    ) -> Vec<ActorEvidence<'e>> {
        let mut actors = Vec::new();
        let mut next_frame_local_index = 0;

        for input in inputs {
            let candidates = input
                .detections
                .iter()
                .filter(|detection| matches_kind(input.kind, detection))
                .filter(|detection| valid_bbox(detection.bbox))
                .collect::<Vec<_>>();

            if candidates.is_empty() {
                continue;
            }

            if let Some(target) = input.target {
                let Some(candidate) = candidates.into_iter().max_by(|left, right| {
                    candidate_rank(left, target.bbox)
                        .cmp_partial(&candidate_rank(right, target.bbox))
                }) else {
                    continue;
                };
                let target_bbox = if valid_bbox(target.bbox) {
                    target.bbox
                } else {
                    candidate.bbox
                };
                let actor_index = match target.id {
                    Some(actor_id) => actors
                        .iter()
                        .position(|actor: &ActorEvidence<'e>| {
                            actor.actor_ref == ActorRef::Track(actor_id)
                        })
                        .unwrap_or_else(|| {
                            actors.push(ActorEvidence::new(ActorRef::Track(actor_id), target_bbox));
                            actors.len() - 1
                        }),
                    None => find_frame_local_actor(
                        &actors,
                        target_bbox,
                        candidate.class.as_str(),
                        self.config,
                    )
                    .unwrap_or_else(|| {
                        let index = actors.len();
                        actors.push(ActorEvidence::new(
                            ActorRef::FrameLocal {
                                frame_number,
                                index: next_frame_local_index,
                            },
                            target_bbox,
                        ));
                        next_frame_local_index += 1;
                        index
                    }),
                };
                self.store_candidate(&mut actors[actor_index], input, candidate, target_bbox);
                continue;
            }

            // Root outputs have no cascade target. Each detection is therefore
            // a frame-local candidate; spatial grouping is limited to this
            // frame and never becomes temporal identity.
            for candidate in candidates {
                let actor_index = find_frame_local_actor(
                    &actors,
                    candidate.bbox,
                    candidate.class.as_str(),
                    self.config,
                )
                .unwrap_or_else(|| {
                    let index = actors.len();
                    actors.push(ActorEvidence::new(
                        ActorRef::FrameLocal {
                            frame_number,
                            index: next_frame_local_index,
                        },
                        candidate.bbox,
                    ));
                    next_frame_local_index += 1;
                    index
                });
                self.store_candidate(&mut actors[actor_index], input, candidate, candidate.bbox);
            }
        }

        actors
    }

    fn store_candidate<'e>(
        &self,
        actor: &mut ActorEvidence<'e>,
        input: &PendingBodyPartsEvidence<'e>,
        candidate: &'e Detection,
        target_bbox: [f32; 4],
    ) {
        let source = SourceDetection {
            model_key: input.model_key,
            detection: candidate,
        };
        match input.kind {
            EvidenceKind::Face => choose_candidate(&mut actor.face, source, target_bbox),
            EvidenceKind::Pose => choose_candidate(&mut actor.pose, source, target_bbox),
            EvidenceKind::Segment => choose_candidate(&mut actor.segment, source, target_bbox),
            EvidenceKind::Detection => {}
        }
    }

    fn estimate_actor(
        &self,
        actor: ActorEvidence<'_>,
        validation_quality: Option<f32>,
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
    ) -> Option<BodyPartsEstimate> {
        let actor_bbox = actor
            .target_bbox
            .is_valid_or(actor.pose.map(|source| source.detection.bbox))
            .or_else(|| actor.face.map(|source| source.detection.bbox))
            .unwrap_or([0.0, 0.0, frame_width as f32, frame_height as f32]);
        let mask_polygons = actor.segment.and_then(|source| {
            source
                .detection
                .mask
                .as_ref()
                .map(|mask| mask.polygons.as_ref().clone())
        });
        let mut parts = Vec::new();

        if let Some(part) = self.estimate_head(
            actor.face,
            actor.pose,
            actor.segment,
            actor_bbox,
            validation_quality,
            frame_number,
            frame_width,
            frame_height,
        ) {
            parts.push(part);
        }
        if let Some(part) = self.estimate_torso(
            actor.pose,
            actor.segment,
            actor_bbox,
            validation_quality,
            frame_number,
            frame_width,
            frame_height,
        ) {
            parts.push(part);
        }
        for (part, indices) in LIMB_JOINTS {
            if let Some(part_estimate) = self.estimate_limb(
                part,
                actor.pose,
                actor.segment,
                actor_bbox,
                indices,
                validation_quality,
                frame_number,
                frame_width,
                frame_height,
            ) {
                parts.push(part_estimate);
            }
        }

        if parts.is_empty() {
            return None;
        }

        // Missing parts are intentionally reflected in the overall score. A
        // partial person is useful diagnostic evidence, but must not look like
        // a complete six-part estimate.
        let local_quality =
            parts.iter().map(|part| part.quality).sum::<f32>() / BodyPartKind::ALL.len() as f32;
        Some(BodyPartsEstimate {
            actor_ref: actor.actor_ref,
            frame_number,
            parts,
            overall_quality: clamp01(local_quality),
            mask_polygons,
        })
    }

    fn estimate_head(
        &self,
        face: Option<SourceDetection<'_>>,
        pose: Option<SourceDetection<'_>>,
        segment: Option<SourceDetection<'_>>,
        actor_bbox: [f32; 4],
        validation_quality: Option<f32>,
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
    ) -> Option<BodyPartEstimate> {
        let head_joints = pose
            .map(|source| self.joints(source.detection, HEAD_JOINTS))
            .unwrap_or_default();
        let face_bbox = face
            .filter(|source| valid_bbox(source.detection.bbox))
            .map(|source| source.detection.bbox);
        if face_bbox.is_none() && head_joints.is_empty() {
            return None;
        }

        let geometry = face_bbox.map(BodyGeometry::Bbox).or_else(|| {
            let points = head_joints
                .iter()
                .map(|joint| joint.point)
                .collect::<Vec<_>>();
            bbox_from_points(
                &points,
                part_radius(
                    actor_bbox,
                    self.config.head_padding_ratio,
                    self.config.minimum_geometry_extent_px,
                ),
                self.config.minimum_geometry_extent_px,
            )
            .map(BodyGeometry::Bbox)
        })?;
        let pose_quality = (!head_joints.is_empty())
            .then(|| joint_quality(&head_joints, HEAD_JOINTS.len()))
            .unwrap_or(0.0);
        let face_quality = face
            .map(|source| clamp01(source.detection.confidence))
            .unwrap_or(0.0);
        let base_quality = match (face_bbox.is_some(), !head_joints.is_empty()) {
            (true, true) => weighted_average(
                face_quality,
                pose_quality,
                self.config.head_face_weight,
                self.config.head_pose_weight,
            ),
            (true, false) => face_quality,
            (false, true) => pose_quality,
            (false, false) => 0.0,
        };
        let mut supports = Vec::new();
        let mut models = Vec::new();
        if face_bbox.is_some() {
            supports.push(BodyPartSupport::Face);
            if let Some(source) = face {
                models.push(source.model_key.to_owned());
            }
        }
        if !head_joints.is_empty() {
            supports.push(BodyPartSupport::Pose);
            if let Some(source) = pose {
                models.push(source.model_key.to_owned());
            }
        }
        Some(self.finalize_part(
            BodyPartKind::Head,
            geometry,
            base_quality,
            supports,
            models,
            segment,
            validation_quality,
            frame_number,
            frame_width,
            frame_height,
        ))
    }

    fn estimate_torso(
        &self,
        pose: Option<SourceDetection<'_>>,
        segment: Option<SourceDetection<'_>>,
        actor_bbox: [f32; 4],
        validation_quality: Option<f32>,
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
    ) -> Option<BodyPartEstimate> {
        let pose = pose?;
        let joints = self.joints(pose.detection, TORSO_JOINTS);
        if joints.is_empty() {
            return None;
        }
        let geometry = torso_geometry(
            &joints,
            actor_bbox,
            self.config.segment_radius_ratio,
            self.config.torso_radius_multiplier,
            self.config.minimum_geometry_extent_px,
        );
        let base_quality =
            joint_quality(&joints, TORSO_JOINTS.len()) * clamp01(pose.detection.confidence);
        Some(self.finalize_part(
            BodyPartKind::Torso,
            geometry,
            base_quality,
            vec![BodyPartSupport::Pose],
            vec![pose.model_key.to_owned()],
            segment,
            validation_quality,
            frame_number,
            frame_width,
            frame_height,
        ))
    }

    fn estimate_limb(
        &self,
        part: BodyPartKind,
        pose: Option<SourceDetection<'_>>,
        segment: Option<SourceDetection<'_>>,
        actor_bbox: [f32; 4],
        indices: [usize; 3],
        validation_quality: Option<f32>,
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
    ) -> Option<BodyPartEstimate> {
        let pose = pose?;
        let joints = self.joints(pose.detection, indices);
        if joints.is_empty() {
            return None;
        }
        let geometry = BodyGeometry::Polyline {
            points: joints.iter().map(|joint| joint.point).collect(),
            radius: part_radius(
                actor_bbox,
                self.config.segment_radius_ratio,
                self.config.minimum_geometry_extent_px,
            ),
        };
        let base_quality =
            joint_quality(&joints, indices.len()) * clamp01(pose.detection.confidence);
        Some(self.finalize_part(
            part,
            geometry,
            base_quality,
            vec![BodyPartSupport::Pose],
            vec![pose.model_key.to_owned()],
            segment,
            validation_quality,
            frame_number,
            frame_width,
            frame_height,
        ))
    }

    fn finalize_part(
        &self,
        part: BodyPartKind,
        geometry: BodyGeometry,
        base_quality: f32,
        mut support: Vec<BodyPartSupport>,
        mut source_models: Vec<String>,
        segment: Option<SourceDetection<'_>>,
        validation_quality: Option<f32>,
        frame_number: u64,
        frame_width: u32,
        frame_height: u32,
    ) -> BodyPartEstimate {
        let mut quality = clamp01(base_quality);
        let mask_coverage = segment.and_then(|source| {
            let coverage = source.detection.mask.as_ref().and_then(|mask| {
                mask_coverage(
                    &geometry,
                    mask,
                    frame_width,
                    frame_height,
                    self.config.geometry_epsilon,
                )
            })?;
            support.push(BodyPartSupport::Segment);
            source_models.push(source.model_key.to_owned());
            Some(coverage)
        });
        if let (Some(coverage), Some(source)) = (mask_coverage, segment) {
            let mask_weight = self.config.mask_quality_weight;
            quality = (1.0 - mask_weight) * quality
                + mask_weight * coverage * clamp01(source.detection.confidence);
        }
        if let Some(cross_quality) = validation_quality {
            let cross_weight = self.config.cross_model_quality_weight;
            quality = (1.0 - cross_weight) * quality + cross_weight * clamp01(cross_quality);
        }
        support.sort_unstable();
        support.dedup();
        source_models.sort_unstable();
        source_models.dedup();
        BodyPartEstimate {
            part,
            geometry,
            support,
            source_models,
            quality: clamp01(quality),
            mask_coverage,
            depth: None,
            source_frame_numbers: vec![frame_number],
            stale: false,
        }
    }

    fn joints(
        &self,
        detection: &Detection,
        indices: impl IntoIterator<Item = usize>,
    ) -> Vec<Joint> {
        let Some(keypoints) = detection.keypoints.as_deref() else {
            return Vec::new();
        };
        indices
            .into_iter()
            .filter_map(|index| keypoints.get(index).copied())
            .filter_map(|[x, y, confidence]| {
                (x.is_finite()
                    && y.is_finite()
                    && confidence.is_finite()
                    && confidence >= self.config.joint_min_confidence)
                    .then_some(Joint {
                        point: [x, y],
                        confidence: clamp01(confidence),
                    })
            })
            .collect()
    }
}

/// Samples one depth map over every body-part footprint in an estimate.
///
/// Pose-derived geometry defines the footprint and the segmentation contours
/// clip it to the visible person. This keeps depth evidence conservative when
/// a limb geometry crosses the background or the bed.
pub(crate) fn attach_depth(
    estimate: &mut BodyPartsEstimate,
    source_model: &str,
    depth: &DepthFrame,
    roi: [u32; 4],
    frame_width: u32,
    frame_height: u32,
) {
    let clip_polygons = estimate.mask_polygons.as_deref();
    for part in &mut estimate.parts {
        let footprints = depth_footprints(&part.geometry);
        let footprint_refs = footprints.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let Some(stats) = mana_perception::polygon_stats(
            depth,
            roi,
            &footprint_refs,
            frame_width,
            frame_height,
            clip_polygons,
        ) else {
            continue;
        };
        let (map_width, map_height) = depth.dims();
        part.depth = Some(BodyPartDepth {
            source_model: source_model.to_owned(),
            roi: stats.roi,
            map_width,
            map_height,
            sampled_pixels: stats.sampled_pixels,
            valid_pixels: stats.valid_pixels,
            valid_ratio: stats.valid_ratio,
            min_depth_m: stats.min_depth_m,
            median_depth_m: stats.median_depth_m,
            p10_depth_m: stats.p10_depth_m,
            p90_depth_m: stats.p90_depth_m,
            max_depth_m: stats.max_depth_m,
            relative_to_torso_m: None,
            surface_evidence: Vec::new(),
        });
    }

    let torso_depth = estimate
        .parts
        .iter()
        .find(|part| part.part == BodyPartKind::Torso)
        .and_then(|part| part.depth.as_ref())
        .and_then(|depth| depth.median_depth_m);
    for part in &mut estimate.parts {
        let Some(depth) = part.depth.as_mut() else {
            continue;
        };
        depth.relative_to_torso_m = match (part.part, torso_depth, depth.median_depth_m) {
            (BodyPartKind::Torso, Some(_), Some(_)) => Some(0.0),
            (_, Some(torso), Some(part_depth)) => Some(part_depth - torso),
            _ => None,
        };
    }
}

/// Compares body-part footprints with fixed bed/floor reference zones.
///
/// This always samples the scene depth map. Person-crop depth remains local to
/// the actor and must not be used as a surface reference.
pub(crate) fn attach_surface_evidence(
    estimates: &mut [BodyPartsEstimate],
    source_model: &str,
    depth: &DepthFrame,
    roi: [u32; 4],
    frame_width: u32,
    frame_height: u32,
    calibration: &SurfaceCalibration,
) {
    if source_model != calibration.model_key
        || frame_width != calibration.frame_width
        || frame_height != calibration.frame_height
        || roi != calibration.roi
    {
        return;
    }

    for estimate in estimates {
        let mask_clips = estimate.mask_polygons.as_deref();
        for part in &mut estimate.parts {
            let footprints = depth_footprints(&part.geometry);
            let footprint_refs = footprints.iter().map(Vec::as_slice).collect::<Vec<_>>();
            if footprint_refs.is_empty() {
                continue;
            }
            let scene_stats = part.depth.is_none().then(|| {
                mana_perception::polygon_stats(
                    depth,
                    roi,
                    &footprint_refs,
                    frame_width,
                    frame_height,
                    mask_clips,
                )
            });
            let mut evidence = Vec::new();
            for (layer, zones) in [
                (SurfaceLayer::Bed, calibration.zones(SurfaceLayer::Bed)),
                (SurfaceLayer::Floor, calibration.zones(SurfaceLayer::Floor)),
            ] {
                let mut best: Option<(u64, SurfaceDepthEvidence)> = None;
                for zone in zones {
                    let normalized_zone = zone
                        .polygon
                        .iter()
                        .map(|[x, y]| [*x / frame_width as f32, *y / frame_height as f32])
                        .collect::<Vec<_>>();
                    let clips = vec![normalized_zone];
                    let mut clip_groups = Vec::with_capacity(2);
                    if let Some(mask_clips) = mask_clips {
                        clip_groups.push(mask_clips);
                    }
                    clip_groups.push(clips.as_slice());
                    let Some(stats) = polygon_stats_intersecting_clips(
                        depth,
                        roi,
                        &footprint_refs,
                        frame_width,
                        frame_height,
                        &clip_groups,
                    ) else {
                        continue;
                    };
                    let Some(observed_median) = stats.median_depth_m else {
                        continue;
                    };
                    let Some(surface_match) = zone.matches(observed_median, 0.0) else {
                        continue;
                    };
                    let candidate = SurfaceDepthEvidence {
                        source_model: source_model.to_owned(),
                        surface: layer.as_str().to_owned(),
                        zone: zone.name.clone(),
                        sampled_pixels: stats.sampled_pixels,
                        valid_ratio: stats.valid_ratio,
                        observed_median: Some(observed_median),
                        reference_median: zone.median_depth,
                        residual: Some(surface_match.residual),
                        in_envelope: surface_match.in_envelope,
                    };
                    if best
                        .as_ref()
                        .is_none_or(|(sampled_pixels, _)| stats.sampled_pixels > *sampled_pixels)
                    {
                        best = Some((stats.sampled_pixels, candidate));
                    }
                }
                if let Some((_, candidate)) = best {
                    evidence.push(candidate);
                }
            }
            if let Some(depth_evidence) = part.depth.as_mut() {
                depth_evidence.surface_evidence = evidence;
            } else if let Some(Some(stats)) = scene_stats {
                let (map_width, map_height) = depth.dims();
                part.depth = Some(BodyPartDepth {
                    source_model: source_model.to_owned(),
                    roi: stats.roi,
                    map_width,
                    map_height,
                    sampled_pixels: stats.sampled_pixels,
                    valid_pixels: stats.valid_pixels,
                    valid_ratio: stats.valid_ratio,
                    min_depth_m: stats.min_depth_m,
                    median_depth_m: stats.median_depth_m,
                    p10_depth_m: stats.p10_depth_m,
                    p90_depth_m: stats.p90_depth_m,
                    max_depth_m: stats.max_depth_m,
                    relative_to_torso_m: None,
                    surface_evidence: evidence,
                });
            }
        }
    }
}

fn depth_footprints(geometry: &BodyGeometry) -> Vec<Vec<[f32; 2]>> {
    match geometry {
        BodyGeometry::Bbox([x1, y1, x2, y2]) => {
            let min_x = x1.min(*x2);
            let max_x = x1.max(*x2);
            let min_y = y1.min(*y2);
            let max_y = y1.max(*y2);
            (min_x.is_finite()
                && max_x.is_finite()
                && min_y.is_finite()
                && max_y.is_finite()
                && max_x > min_x
                && max_y > min_y)
                .then(|| {
                    vec![vec![
                        [min_x, min_y],
                        [max_x, min_y],
                        [max_x, max_y],
                        [min_x, max_y],
                    ]]
                })
                .unwrap_or_default()
        }
        BodyGeometry::Polygon(points) => (points.len() >= 3
            && points.iter().all(|[x, y]| x.is_finite() && y.is_finite()))
        .then_some(vec![points.clone()])
        .unwrap_or_default(),
        BodyGeometry::Polyline { points, radius } => {
            if points.is_empty() || !radius.is_finite() || *radius <= 0.0 {
                return Vec::new();
            }
            let radius = (*radius).max(1.0);
            let mut footprints = points
                .windows(2)
                .filter_map(|pair| capsule_polygon(pair[0], pair[1], radius))
                .collect::<Vec<_>>();
            for &point in points {
                if point[0].is_finite() && point[1].is_finite() {
                    footprints.push(circle_polygon(point, radius));
                }
            }
            footprints
        }
    }
}

fn capsule_polygon(start: [f32; 2], end: [f32; 2], radius: f32) -> Option<Vec<[f32; 2]>> {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length = (dx * dx + dy * dy).sqrt();
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    let nx = -dy / length * radius;
    let ny = dx / length * radius;
    Some(vec![
        [start[0] + nx, start[1] + ny],
        [end[0] + nx, end[1] + ny],
        [end[0] - nx, end[1] - ny],
        [start[0] - nx, start[1] - ny],
    ])
}

fn circle_polygon(center: [f32; 2], radius: f32) -> Vec<[f32; 2]> {
    const SIDES: usize = 8;
    (0..SIDES)
        .map(|index| {
            #[allow(clippy::cast_precision_loss)]
            let angle = std::f32::consts::TAU * index as f32 / SIDES as f32;
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]
        })
        .collect()
}

fn overall_quality(parts: &[BodyPartEstimate]) -> f32 {
    parts.iter().map(|part| part.quality).sum::<f32>() / BodyPartKind::ALL.len() as f32
}

fn complete_geometry(part: BodyPartKind, geometry: &BodyGeometry) -> bool {
    match part {
        BodyPartKind::Head => match geometry {
            BodyGeometry::Bbox(_) => true,
            BodyGeometry::Polygon(points) | BodyGeometry::Polyline { points, .. } => {
                !points.is_empty()
            }
        },
        BodyPartKind::Torso => {
            matches!(geometry, BodyGeometry::Polygon(points) if points.len() >= 4)
        }
        BodyPartKind::LeftArm
        | BodyPartKind::RightArm
        | BodyPartKind::LeftLeg
        | BodyPartKind::RightLeg => {
            matches!(geometry, BodyGeometry::Polyline { points, .. } if points.len() >= 3)
        }
    }
}

fn mask_supports_prediction(coverage: Option<f32>, threshold: f32) -> bool {
    coverage.is_some_and(|coverage| coverage >= threshold)
}

fn add_support(support: &mut Vec<BodyPartSupport>, value: BodyPartSupport) {
    support.push(value);
    support.sort_unstable();
    support.dedup();
}

fn merge_source_models(left: &mut Vec<String>, right: &[String]) {
    left.extend(right.iter().cloned());
    left.sort_unstable();
    left.dedup();
}

fn merge_frame_numbers(left: &mut Vec<u64>, right: &[u64], current_frame: u64) {
    left.extend(right.iter().copied());
    left.push(current_frame);
    left.sort_unstable();
    left.dedup();
}

fn apply_validation_quality(
    quality: &mut f32,
    validation_quality: Option<f32>,
    config: &BodyPartsConfig,
) {
    if let Some(cross_quality) = validation_quality {
        let cross_weight = config.cross_model_quality_weight;
        *quality = (1.0 - cross_weight) * *quality + cross_weight * clamp01(cross_quality);
    }
    *quality = clamp01(*quality);
}

fn transform_geometry(
    geometry: &BodyGeometry,
    previous_bbox: [f32; 4],
    current_bbox: [f32; 4],
) -> BodyGeometry {
    if !valid_bbox(previous_bbox) || !valid_bbox(current_bbox) {
        return geometry.clone();
    }
    let map = |point: [f32; 2]| {
        [
            current_bbox[0]
                + (point[0] - previous_bbox[0]) / (previous_bbox[2] - previous_bbox[0])
                    * (current_bbox[2] - current_bbox[0]),
            current_bbox[1]
                + (point[1] - previous_bbox[1]) / (previous_bbox[3] - previous_bbox[1])
                    * (current_bbox[3] - current_bbox[1]),
        ]
    };
    let scale_x = (current_bbox[2] - current_bbox[0]) / (previous_bbox[2] - previous_bbox[0]);
    let scale_y = (current_bbox[3] - current_bbox[1]) / (previous_bbox[3] - previous_bbox[1]);
    let scale = ((scale_x.abs() + scale_y.abs()) / 2.0).max(f32::EPSILON);
    match geometry {
        BodyGeometry::Bbox([x1, y1, x2, y2]) => {
            let [min_x, min_y] = map([*x1, *y1]);
            let [max_x, max_y] = map([*x2, *y2]);
            BodyGeometry::Bbox([min_x, min_y, max_x, max_y])
        }
        BodyGeometry::Polygon(points) => {
            BodyGeometry::Polygon(points.iter().copied().map(map).collect())
        }
        BodyGeometry::Polyline { points, radius } => BodyGeometry::Polyline {
            points: points.iter().copied().map(map).collect(),
            radius: *radius * scale,
        },
    }
}

fn blend_geometry(
    current: &BodyGeometry,
    previous: &BodyGeometry,
    current_weight: f32,
) -> BodyGeometry {
    let current_weight = clamp01(current_weight);
    let blend_point = |left: [f32; 2], right: [f32; 2]| {
        [
            left[0] * current_weight + right[0] * (1.0 - current_weight),
            left[1] * current_weight + right[1] * (1.0 - current_weight),
        ]
    };
    match (current, previous) {
        (BodyGeometry::Bbox(current), BodyGeometry::Bbox(previous)) => BodyGeometry::Bbox([
            current[0] * current_weight + previous[0] * (1.0 - current_weight),
            current[1] * current_weight + previous[1] * (1.0 - current_weight),
            current[2] * current_weight + previous[2] * (1.0 - current_weight),
            current[3] * current_weight + previous[3] * (1.0 - current_weight),
        ]),
        (BodyGeometry::Polygon(current), BodyGeometry::Polygon(previous))
            if current.len() == previous.len() =>
        {
            BodyGeometry::Polygon(
                current
                    .iter()
                    .copied()
                    .zip(previous.iter().copied())
                    .map(|(current, previous)| blend_point(current, previous))
                    .collect(),
            )
        }
        (
            BodyGeometry::Polyline {
                points: current_points,
                radius: current_radius,
            },
            BodyGeometry::Polyline {
                points: previous_points,
                radius: previous_radius,
            },
        ) if current_points.len() == previous_points.len() => BodyGeometry::Polyline {
            points: current_points
                .iter()
                .copied()
                .zip(previous_points.iter().copied())
                .map(|(current, previous)| blend_point(current, previous))
                .collect(),
            radius: current_radius * current_weight + previous_radius * (1.0 - current_weight),
        },
        _ => current.clone(),
    }
}

fn matches_kind(kind: EvidenceKind, detection: &Detection) -> bool {
    match kind {
        EvidenceKind::Face => detection.class == "face",
        EvidenceKind::Detection | EvidenceKind::Pose | EvidenceKind::Segment => {
            detection.class == "person"
        }
    }
}

fn choose_candidate<'a>(
    current: &mut Option<SourceDetection<'a>>,
    candidate: SourceDetection<'a>,
    target_bbox: [f32; 4],
) {
    let replace = current
        .map(|existing| {
            candidate_rank(candidate.detection, target_bbox)
                .cmp_partial(&candidate_rank(existing.detection, target_bbox))
                .is_gt()
        })
        .unwrap_or(true);
    if replace {
        *current = Some(candidate);
    }
}

fn find_frame_local_actor<'a>(
    actors: &[ActorEvidence<'a>],
    bbox: [f32; 4],
    class: &str,
    config: &BodyPartsConfig,
) -> Option<usize> {
    actors.iter().position(|actor| {
        if !matches!(actor.actor_ref, ActorRef::FrameLocal { .. }) {
            return false;
        }
        let iou = bbox_iou(actor.target_bbox, bbox);
        iou >= config.frame_local_match_iou
            || (class == "face"
                && (bbox_coverage(bbox, actor.target_bbox) >= config.face_frame_local_coverage
                    || bbox_coverage(actor.target_bbox, bbox) >= config.face_frame_local_coverage))
    })
}

fn candidate_rank(detection: &Detection, target_bbox: [f32; 4]) -> CandidateRank {
    CandidateRank {
        valid: valid_bbox(detection.bbox),
        overlap: bbox_iou(detection.bbox, target_bbox),
        confidence: clamp01(detection.confidence),
    }
}

#[derive(Debug, Clone, Copy)]
struct CandidateRank {
    valid: bool,
    overlap: f32,
    confidence: f32,
}

impl CandidateRank {
    fn cmp_partial(self, other: &Self) -> std::cmp::Ordering {
        self.valid
            .cmp(&other.valid)
            .then_with(|| self.overlap.total_cmp(&other.overlap))
            .then_with(|| self.confidence.total_cmp(&other.confidence))
    }
}

fn torso_geometry(
    joints: &[Joint],
    actor_bbox: [f32; 4],
    radius_ratio: f32,
    radius_multiplier: f32,
    minimum_extent: f32,
) -> BodyGeometry {
    let points = joints.iter().map(|joint| joint.point).collect::<Vec<_>>();
    match points.len() {
        1 => BodyGeometry::Bbox(
            bbox_from_points(
                &points,
                part_radius(actor_bbox, radius_ratio * radius_multiplier, minimum_extent),
                minimum_extent,
            )
            .unwrap_or([
                points[0][0],
                points[0][1],
                points[0][0] + minimum_extent,
                points[0][1] + minimum_extent,
            ]),
        ),
        2 => BodyGeometry::Polyline {
            points,
            radius: part_radius(actor_bbox, radius_ratio * radius_multiplier, minimum_extent),
        },
        _ => BodyGeometry::Polygon(points),
    }
}

fn joint_quality(joints: &[Joint], expected: usize) -> f32 {
    if joints.is_empty() || expected == 0 {
        return 0.0;
    }
    let confidence = joints.iter().map(|joint| joint.confidence).sum::<f32>() / joints.len() as f32;
    clamp01(confidence * (joints.len() as f32 / expected as f32))
}

fn mask_coverage(
    geometry: &BodyGeometry,
    mask: &crate::detection::DetectionMask,
    frame_width: u32,
    frame_height: u32,
    geometry_epsilon: f32,
) -> Option<f32> {
    if mask.polygons.is_empty() || frame_width == 0 || frame_height == 0 {
        return None;
    }
    let points = geometry_points(geometry);
    if points.is_empty() {
        return None;
    }
    let inside = points
        .iter()
        .filter(|[x, y]| {
            let normalized = [*x / frame_width as f32, *y / frame_height as f32];
            mask.polygons
                .iter()
                .any(|polygon| point_in_polygon(normalized, polygon, geometry_epsilon))
        })
        .count();
    Some(clamp01(inside as f32 / points.len() as f32))
}

fn geometry_points(geometry: &BodyGeometry) -> Vec<[f32; 2]> {
    match geometry {
        BodyGeometry::Bbox([x1, y1, x2, y2]) => vec![
            [*x1, *y1],
            [*x2, *y1],
            [*x2, *y2],
            [*x1, *y2],
            [(*x1 + *x2) / 2.0, (*y1 + *y2) / 2.0],
        ],
        BodyGeometry::Polygon(points) | BodyGeometry::Polyline { points, .. } => points.clone(),
    }
}

fn point_in_polygon(point: [f32; 2], polygon: &[[f32; 2]], geometry_epsilon: f32) -> bool {
    if polygon.len() < 3 || !point[0].is_finite() || !point[1].is_finite() {
        return false;
    }
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        if point_on_segment(point, previous, current, geometry_epsilon) {
            return true;
        }
        let crosses = (current[1] > point[1]) != (previous[1] > point[1]);
        if crosses {
            let denominator = previous[1] - current[1];
            if denominator.abs() > geometry_epsilon {
                let x_at_y =
                    (previous[0] - current[0]) * (point[1] - current[1]) / denominator + current[0];
                if point[0] < x_at_y {
                    inside = !inside;
                }
            }
        }
        previous = current;
    }
    inside
}

fn point_on_segment(
    point: [f32; 2],
    start: [f32; 2],
    end: [f32; 2],
    geometry_epsilon: f32,
) -> bool {
    if !start[0].is_finite() || !start[1].is_finite() || !end[0].is_finite() || !end[1].is_finite()
    {
        return false;
    }
    let cross =
        (point[1] - start[1]) * (end[0] - start[0]) - (point[0] - start[0]) * (end[1] - start[1]);
    if cross.abs() > geometry_epsilon {
        return false;
    }
    point[0] >= start[0].min(end[0]) - geometry_epsilon
        && point[0] <= start[0].max(end[0]) + geometry_epsilon
        && point[1] >= start[1].min(end[1]) - geometry_epsilon
        && point[1] <= start[1].max(end[1]) + geometry_epsilon
}

fn bbox_from_points(points: &[[f32; 2]], padding: f32, minimum_extent: f32) -> Option<[f32; 4]> {
    let mut valid = points
        .iter()
        .filter(|[x, y]| x.is_finite() && y.is_finite());
    let first = *valid.next()?;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (first[0], first[1], first[0], first[1]);
    for [x, y] in valid {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }
    let minimum_extent = if minimum_extent.is_finite() {
        minimum_extent.max(f32::EPSILON)
    } else {
        f32::EPSILON
    };
    let padding = if padding.is_finite() {
        padding.max(minimum_extent)
    } else {
        minimum_extent
    };
    Some([
        min_x - padding,
        min_y - padding,
        max_x.max(min_x + minimum_extent) + padding,
        max_y.max(min_y + minimum_extent) + padding,
    ])
}

fn part_radius(bbox: [f32; 4], ratio: f32, minimum_extent: f32) -> f32 {
    let width = (bbox[2] - bbox[0]).abs();
    let height = (bbox[3] - bbox[1]).abs();
    let minimum_extent = if minimum_extent.is_finite() {
        minimum_extent.max(f32::EPSILON)
    } else {
        f32::EPSILON
    };
    let extent = width.min(height).max(minimum_extent);
    (extent * ratio).max(minimum_extent)
}

fn valid_bbox(bbox: [f32; 4]) -> bool {
    bbox.iter().all(|value| value.is_finite()) && bbox[2] > bbox[0] && bbox[3] > bbox[1]
}

fn bbox_coverage(inner: [f32; 4], outer: [f32; 4]) -> f32 {
    let area = bbox_area(inner);
    if area <= 0.0 {
        return 0.0;
    }
    let x1 = inner[0].max(outer[0]);
    let y1 = inner[1].max(outer[1]);
    let x2 = inner[2].min(outer[2]);
    let y2 = inner[3].min(outer[3]);
    (((x2 - x1).max(0.0) * (y2 - y1).max(0.0)) / area).clamp(0.0, 1.0)
}

fn bbox_iou(left: [f32; 4], right: [f32; 4]) -> f32 {
    let intersection = bbox_coverage(left, right) * bbox_area(left);
    let union = bbox_area(left) + bbox_area(right) - intersection;
    if union > 0.0 {
        (intersection / union).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn bbox_area(bbox: [f32; 4]) -> f32 {
    ((bbox[2] - bbox[0]).max(0.0)) * ((bbox[3] - bbox[1]).max(0.0))
}

fn clamp01(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn weighted_average(left: f32, right: f32, left_weight: f32, right_weight: f32) -> f32 {
    let total = left_weight + right_weight;
    if total > 0.0 {
        clamp01((left * left_weight + right * right_weight) / total)
    } else {
        0.0
    }
}

trait ValidOr {
    fn is_valid_or(self, fallback: Option<[f32; 4]>) -> Option<[f32; 4]>;
}

impl ValidOr for [f32; 4] {
    fn is_valid_or(self, fallback: Option<[f32; 4]>) -> Option<[f32; 4]> {
        if valid_bbox(self) {
            Some(self)
        } else {
            fallback.filter(|bbox| valid_bbox(*bbox))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;
    use std::sync::Arc;
    use ultralytics_inference::DepthMap;

    fn detection(class: &str, bbox: [f32; 4], confidence: f32) -> Detection {
        Detection {
            class: class.into(),
            confidence,
            bbox,
            keypoints: None,
            mask: None,
        }
    }

    fn pose() -> Detection {
        let mut keypoints = vec![[0.0, 0.0, 0.0]; 17];
        keypoints[0] = [200.0, 120.0, 0.95];
        keypoints[1] = [190.0, 110.0, 0.90];
        keypoints[2] = [210.0, 110.0, 0.90];
        keypoints[3] = [180.0, 120.0, 0.85];
        keypoints[4] = [220.0, 120.0, 0.85];
        keypoints[5] = [150.0, 200.0, 0.90];
        keypoints[6] = [250.0, 200.0, 0.90];
        keypoints[7] = [120.0, 280.0, 0.85];
        keypoints[8] = [280.0, 280.0, 0.85];
        keypoints[9] = [100.0, 350.0, 0.80];
        keypoints[10] = [300.0, 350.0, 0.80];
        keypoints[11] = [170.0, 400.0, 0.90];
        keypoints[12] = [230.0, 400.0, 0.90];
        keypoints[13] = [160.0, 500.0, 0.85];
        keypoints[14] = [240.0, 500.0, 0.85];
        keypoints[15] = [150.0, 600.0, 0.80];
        keypoints[16] = [250.0, 600.0, 0.80];
        Detection {
            keypoints: Some(keypoints),
            ..detection("person", [80.0, 80.0, 320.0, 650.0], 0.92)
        }
    }

    fn segment() -> Detection {
        Detection {
            mask: Some(crate::detection::DetectionMask {
                compact: Arc::new(
                    mana_geometry::compact_mask::CompactMask::from_dense(
                        &[1],
                        1,
                        1,
                        (0, 0),
                        (1000, 1000),
                    )
                    .expect("valid compact mask"),
                ),
                polygons: Arc::new(vec![vec![
                    [0.05, 0.05],
                    [0.35, 0.05],
                    [0.35, 0.70],
                    [0.05, 0.70],
                ]]),
                origin: [0, 0],
                mask_dims: [1000, 1000],
            }),
            ..detection("person", [80.0, 80.0, 320.0, 650.0], 0.88)
        }
    }

    fn target(id: Option<u64>) -> CascadeTarget {
        CascadeTarget {
            id,
            bbox: [80.0, 80.0, 320.0, 650.0],
        }
    }

    fn body_parts_config() -> BodyPartsConfig {
        BodyPartsConfig {
            mode: crate::config::BodyPartsMode::Validator,
            frame_local_match_iou: 0.50,
            face_frame_local_coverage: 0.50,
            joint_min_confidence: 0.25,
            segment_radius_ratio: 0.035,
            head_padding_ratio: 0.06,
            torso_radius_multiplier: 1.5,
            head_face_weight: 0.65,
            head_pose_weight: 0.35,
            mask_quality_weight: 0.25,
            cross_model_quality_weight: 0.20,
            minimum_geometry_extent_px: 1.0,
            geometry_epsilon: 0.00001,
            advanced_smoothing_alpha: 0.65,
            advanced_mask_support_threshold: 0.60,
            advanced_max_gap_frames: 3,
            advanced_temporal_quality_decay: 0.85,
        }
    }

    fn advanced_body_parts_config() -> BodyPartsConfig {
        let mut config = body_parts_config();
        config.mode = crate::config::BodyPartsMode::Advanced;
        config
    }

    fn inputs<'a>(
        face: &'a Detection,
        pose: &'a Detection,
        segment: &'a Detection,
        target: Option<CascadeTarget>,
    ) -> [PendingBodyPartsEvidence<'a>; 3] {
        [
            PendingBodyPartsEvidence {
                model_key: "face-yolo",
                kind: EvidenceKind::Face,
                target,
                detections: std::slice::from_ref(face),
            },
            PendingBodyPartsEvidence {
                model_key: "pose-standard",
                kind: EvidenceKind::Pose,
                target,
                detections: std::slice::from_ref(pose),
            },
            PendingBodyPartsEvidence {
                model_key: "seg-standard",
                kind: EvidenceKind::Segment,
                target,
                detections: std::slice::from_ref(segment),
            },
        ]
    }

    #[test]
    fn estimates_six_parts_from_complete_pose() {
        let face = detection("face", [170.0, 90.0, 230.0, 165.0], 0.86);
        let pose = pose();
        let segment = segment();
        let config = body_parts_config();
        let estimate = BodyPartsEstimator::new(&config)
            .estimate(
                &inputs(&face, &pose, &segment, Some(target(Some(7)))),
                &[],
                12,
                1000,
                1000,
            )
            .pop()
            .expect("complete actor estimate");

        assert_eq!(estimate.actor_ref, ActorRef::Track(7));
        assert_eq!(estimate.parts.len(), 6);
        assert!(
            estimate
                .parts
                .iter()
                .all(|part| (0.0..=1.0).contains(&part.quality) && !part.stale)
        );
        assert!(
            estimate
                .parts
                .iter()
                .all(|part| part.source_frame_numbers == vec![12])
        );
        assert!(
            estimate
                .parts
                .iter()
                .any(|part| part.support.contains(&BodyPartSupport::Segment))
        );
    }

    #[test]
    fn depth_is_sampled_inside_part_footprints_and_compared_to_torso() {
        let data = Array2::from_shape_fn((10, 10), |(_, x)| if x < 5 { 2.0 } else { 4.0 });
        let depth = DepthFrame::from_ultralytics(DepthMap::new(data, (10, 10)));
        let mut estimate = BodyPartsEstimate {
            actor_ref: ActorRef::Track(7),
            frame_number: 12,
            parts: vec![
                BodyPartEstimate {
                    part: BodyPartKind::Torso,
                    geometry: BodyGeometry::Polygon(vec![
                        [10.0, 10.0],
                        [40.0, 10.0],
                        [40.0, 40.0],
                        [10.0, 40.0],
                    ]),
                    support: vec![BodyPartSupport::Pose, BodyPartSupport::Segment],
                    source_models: vec!["pose-standard".into(), "seg-standard".into()],
                    quality: 1.0,
                    mask_coverage: Some(1.0),
                    depth: None,
                    source_frame_numbers: vec![12],
                    stale: false,
                },
                BodyPartEstimate {
                    part: BodyPartKind::LeftArm,
                    geometry: BodyGeometry::Polyline {
                        points: vec![[60.0, 10.0], [80.0, 10.0], [90.0, 20.0]],
                        radius: 2.0,
                    },
                    support: vec![BodyPartSupport::Pose, BodyPartSupport::Segment],
                    source_models: vec!["pose-standard".into(), "seg-standard".into()],
                    quality: 1.0,
                    mask_coverage: Some(1.0),
                    depth: None,
                    source_frame_numbers: vec![12],
                    stale: false,
                },
            ],
            overall_quality: 1.0,
            mask_polygons: Some(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]]),
        };

        attach_depth(
            &mut estimate,
            "depth-person-s-320",
            &depth,
            [0, 0, 100, 100],
            100,
            100,
        );

        let torso = estimate
            .parts
            .iter()
            .find(|part| part.part == BodyPartKind::Torso)
            .and_then(|part| part.depth.as_ref())
            .expect("torso depth");
        let arm = estimate
            .parts
            .iter()
            .find(|part| part.part == BodyPartKind::LeftArm)
            .and_then(|part| part.depth.as_ref())
            .expect("arm depth");
        assert_eq!(torso.median_depth_m, Some(2.0));
        assert_eq!(arm.median_depth_m, Some(4.0));
        assert_eq!(arm.relative_to_torso_m, Some(2.0));
        assert_eq!(arm.source_model, "depth-person-s-320");
        assert!(arm.sampled_pixels > 0);
    }

    #[test]
    fn surface_evidence_uses_scene_depth_and_zone_envelope() {
        let data = Array2::from_elem((10, 10), 2.0);
        let depth = DepthFrame::from_ultralytics(DepthMap::new(data, (10, 10)));
        let mut calibration =
            SurfaceCalibration::new("depth-standard".into(), None, 100, 100, [0, 0, 100, 100]);
        calibration
            .upsert_zone(
                SurfaceLayer::Bed,
                mana_perception::SurfaceZone {
                    name: "body".into(),
                    polygon: vec![[0.0, 0.0], [60.0, 0.0], [60.0, 60.0], [0.0, 60.0]],
                    median_depth: 2.0,
                    p10_depth: 1.9,
                    p90_depth: 2.1,
                    mad_depth: 0.0,
                    valid_ratio: 1.0,
                    frame_samples: 3,
                    valid_frames: 3,
                    tolerance: 0.0,
                },
            )
            .expect("valid surface zone");
        let mut estimate = BodyPartsEstimate {
            actor_ref: ActorRef::Track(7),
            frame_number: 12,
            parts: vec![BodyPartEstimate {
                part: BodyPartKind::Torso,
                geometry: BodyGeometry::Polygon(vec![
                    [10.0, 10.0],
                    [40.0, 10.0],
                    [40.0, 40.0],
                    [10.0, 40.0],
                ]),
                support: vec![BodyPartSupport::Pose],
                source_models: vec!["pose-standard".into()],
                quality: 1.0,
                mask_coverage: None,
                depth: None,
                source_frame_numbers: vec![12],
                stale: false,
            }],
            overall_quality: 1.0,
            mask_polygons: None,
        };

        attach_depth(
            &mut estimate,
            "depth-person-s-320",
            &depth,
            [0, 0, 100, 100],
            100,
            100,
        );
        attach_surface_evidence(
            std::slice::from_mut(&mut estimate),
            "depth-standard",
            &depth,
            [0, 0, 100, 100],
            100,
            100,
            &calibration,
        );

        let evidence = &estimate.parts[0]
            .depth
            .as_ref()
            .expect("person depth")
            .surface_evidence[0];
        assert_eq!(evidence.source_model, "depth-standard");
        assert_eq!(evidence.surface, "bed");
        assert_eq!(evidence.zone, "body");
        assert_eq!(evidence.observed_median, Some(2.0));
        assert_eq!(evidence.residual, Some(0.0));
        assert!(evidence.in_envelope);
    }

    #[test]
    fn missing_joints_produce_partial_parts() {
        let face = detection("face", [170.0, 90.0, 230.0, 165.0], 0.86);
        let mut pose = pose();
        for index in [8, 10, 14, 16] {
            pose.keypoints.as_mut().unwrap()[index][2] = 0.1;
        }
        let segment = segment();
        let config = body_parts_config();
        let estimate = BodyPartsEstimator::new(&config)
            .estimate(
                &inputs(&face, &pose, &segment, Some(target(Some(7)))),
                &[],
                12,
                1000,
                1000,
            )
            .pop()
            .expect("partial actor estimate");

        assert!(
            estimate
                .parts
                .iter()
                .any(|part| part.part == BodyPartKind::Head)
        );
        assert!(
            estimate
                .parts
                .iter()
                .any(|part| part.part == BodyPartKind::RightArm && part.quality < 0.6)
        );
        assert_eq!(estimate.parts.len(), 6);
    }

    #[test]
    fn advanced_mode_recovers_mask_supported_missing_limb_from_track_history() {
        let face = detection("face", [170.0, 90.0, 230.0, 165.0], 0.86);
        let complete_pose = pose();
        let segment = segment();
        let config = advanced_body_parts_config();
        let mut temporal = BodyPartsTemporalState::default();
        let first = BodyPartsEstimator::new(&config)
            .estimate_advanced(
                &inputs(&face, &complete_pose, &segment, Some(target(Some(7)))),
                &[],
                12,
                1000,
                1000,
                &mut temporal,
            )
            .pop()
            .expect("initial advanced estimate");
        assert!(
            first
                .parts
                .iter()
                .find(|part| part.part == BodyPartKind::RightArm)
                .is_some_and(|part| !part.stale)
        );

        let mut missing_pose = pose();
        for index in [6, 8, 10] {
            missing_pose.keypoints.as_mut().unwrap()[index][2] = 0.1;
        }
        let recovered = BodyPartsEstimator::new(&config)
            .estimate_advanced(
                &inputs(&face, &missing_pose, &segment, Some(target(Some(7)))),
                &[],
                13,
                1000,
                1000,
                &mut temporal,
            )
            .pop()
            .expect("advanced estimate with temporal completion");
        let right_arm = recovered
            .parts
            .iter()
            .find(|part| part.part == BodyPartKind::RightArm)
            .expect("right arm recovered");
        assert!(right_arm.stale);
        assert!(right_arm.support.contains(&BodyPartSupport::Temporal));
        assert_eq!(geometry_points(&right_arm.geometry).len(), 3);
        assert!(right_arm.mask_coverage.unwrap_or(0.0) >= 0.60);
        assert_eq!(right_arm.source_frame_numbers, vec![12, 13]);
    }

    #[test]
    fn advanced_mode_does_not_reuse_history_without_mask_support() {
        let face = detection("face", [170.0, 90.0, 230.0, 165.0], 0.86);
        let complete_pose = pose();
        let initial_segment = segment();
        let config = advanced_body_parts_config();
        let mut temporal = BodyPartsTemporalState::default();
        let _ = BodyPartsEstimator::new(&config).estimate_advanced(
            &inputs(
                &face,
                &complete_pose,
                &initial_segment,
                Some(target(Some(7))),
            ),
            &[],
            12,
            1000,
            1000,
            &mut temporal,
        );

        let mut unsupported_segment = segment();
        unsupported_segment.mask.as_mut().unwrap().polygons = Arc::new(vec![vec![
            [0.70, 0.70],
            [0.90, 0.70],
            [0.90, 0.90],
            [0.70, 0.90],
        ]]);
        let mut missing_pose = pose();
        for index in [6, 8, 10] {
            missing_pose.keypoints.as_mut().unwrap()[index][2] = 0.1;
        }
        let estimate = BodyPartsEstimator::new(&config)
            .estimate_advanced(
                &inputs(
                    &face,
                    &missing_pose,
                    &unsupported_segment,
                    Some(target(Some(7))),
                ),
                &[],
                13,
                1000,
                1000,
                &mut temporal,
            )
            .pop()
            .expect("partial estimate remains available");
        let right_arm = estimate
            .parts
            .iter()
            .find(|part| part.part == BodyPartKind::RightArm);
        assert!(right_arm.is_none() || !right_arm.unwrap().stale);
    }

    #[test]
    fn advanced_mode_smooths_complete_track_geometry() {
        let face = detection("face", [170.0, 90.0, 230.0, 165.0], 0.86);
        let segment = segment();
        let config = advanced_body_parts_config();
        let mut temporal = BodyPartsTemporalState::default();
        let first_pose = pose();
        let _ = BodyPartsEstimator::new(&config).estimate_advanced(
            &inputs(&face, &first_pose, &segment, Some(target(Some(7)))),
            &[],
            12,
            1000,
            1000,
            &mut temporal,
        );

        let mut moved_pose = pose();
        for index in [6, 8, 10] {
            moved_pose.keypoints.as_mut().unwrap()[index][0] += 20.0;
        }
        let estimate = BodyPartsEstimator::new(&config)
            .estimate_advanced(
                &inputs(&face, &moved_pose, &segment, Some(target(Some(7)))),
                &[],
                13,
                1000,
                1000,
                &mut temporal,
            )
            .pop()
            .expect("smoothed advanced estimate");
        let right_arm = estimate
            .parts
            .iter()
            .find(|part| part.part == BodyPartKind::RightArm)
            .expect("right arm");
        let BodyGeometry::Polyline { points, .. } = &right_arm.geometry else {
            panic!("right arm must remain a polyline");
        };
        assert!(points[0][0] > 250.0 && points[0][0] < 270.0);
        assert!(right_arm.support.contains(&BodyPartSupport::Temporal));
        assert!(!right_arm.stale);
    }

    #[test]
    fn frame_local_output_has_no_temporal_identity() {
        let face = detection("face", [170.0, 90.0, 230.0, 165.0], 0.86);
        let pose = pose();
        let segment = segment();
        let config = body_parts_config();
        let estimate = BodyPartsEstimator::new(&config)
            .estimate(
                &inputs(&face, &pose, &segment, Some(target(None))),
                &[],
                12,
                1000,
                1000,
            )
            .pop()
            .expect("frame-local actor estimate");

        assert_eq!(
            estimate.actor_ref,
            ActorRef::FrameLocal {
                frame_number: 12,
                index: 0
            }
        );
    }

    #[test]
    fn cross_model_quality_reduces_part_quality_without_invalidating_geometry() {
        let face = detection("face", [170.0, 90.0, 230.0, 165.0], 0.86);
        let pose = pose();
        let segment = segment();
        let inputs = inputs(&face, &pose, &segment, Some(target(Some(7))));
        let config = body_parts_config();
        let baseline = BodyPartsEstimator::new(&config).estimate(&inputs, &[], 12, 1000, 1000);
        let validation = CrossModelValidation {
            actor_id: 7,
            frame_number: 12,
            quality: 0.1,
            agreement: 0.1,
            freshness: 1.0,
            supporting_sources: Vec::new(),
            contradicting_sources: vec!["pose-standard".into(), "seg-standard".into()],
            reasons: vec!["pose_segment=0.1".into()],
        };
        let degraded =
            BodyPartsEstimator::new(&config).estimate(&inputs, &[validation], 12, 1000, 1000);

        assert!(degraded[0].overall_quality < baseline[0].overall_quality);
        assert_eq!(degraded[0].parts.len(), 6);
    }

    #[test]
    fn mask_coverage_does_not_mutate_raw_mask() {
        let face = detection("face", [170.0, 90.0, 230.0, 165.0], 0.86);
        let pose = pose();
        let segment = segment();
        let original_polygons = segment.mask.as_ref().unwrap().polygons.as_ref().clone();
        let inputs = inputs(&face, &pose, &segment, Some(target(Some(7))));
        let config = body_parts_config();
        let _ = BodyPartsEstimator::new(&config).estimate(&inputs, &[], 12, 1000, 1000);

        assert_eq!(
            segment.mask.as_ref().unwrap().polygons.as_ref(),
            &original_polygons
        );
    }
}
