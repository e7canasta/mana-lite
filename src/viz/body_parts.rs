//! Derived body-part geometry as native Rerun 2D line strips.

use crate::app::body_parts::{ActorRef, BodyGeometry, BodyPartKind, BodyPartsEstimate};

use super::{Inner, VizBridge};

const BODY_PARTS_PATH: &str = "/world/camera/body_parts";
const DEFAULT_RADIUS: f32 = 2.0;

fn body_part_color(part: BodyPartKind) -> [u8; 3] {
    match part {
        BodyPartKind::Head => [255, 220, 0],
        BodyPartKind::Torso => [0, 220, 255],
        BodyPartKind::LeftArm => [255, 95, 95],
        BodyPartKind::RightArm => [255, 0, 180],
        BodyPartKind::LeftLeg => [100, 255, 100],
        BodyPartKind::RightLeg => [100, 145, 255],
    }
}

fn body_part_alpha(stale: bool) -> u8 {
    if stale { 140 } else { 235 }
}

fn actor_path(actor: &ActorRef) -> String {
    match actor {
        ActorRef::Track(id) => format!("{BODY_PARTS_PATH}/track/{id}"),
        ActorRef::FrameLocal {
            frame_number,
            index,
        } => format!("{BODY_PARTS_PATH}/frame_local/{frame_number}/{index}"),
    }
}

fn geometry_strip(geometry: &BodyGeometry) -> Option<(Vec<[f32; 2]>, f32)> {
    let (mut points, radius, closed) = match geometry {
        BodyGeometry::Bbox([x1, y1, x2, y2]) => (
            vec![[*x1, *y1], [*x2, *y1], [*x2, *y2], [*x1, *y2]],
            DEFAULT_RADIUS,
            true,
        ),
        BodyGeometry::Polygon(points) => (points.clone(), DEFAULT_RADIUS, true),
        BodyGeometry::Polyline { points, radius } => (points.clone(), *radius, false),
    };

    if points.len() < 2 || points.iter().any(|[x, y]| !x.is_finite() || !y.is_finite()) {
        return None;
    }
    if closed && points.first() != points.last() {
        points.push(points[0]);
    }
    let radius = radius
        .is_finite()
        .then_some(radius.max(1.0))
        .unwrap_or(DEFAULT_RADIUS);
    Some((points, radius))
}

impl VizBridge {
    /// Publishes the same-frame body-part output that is already emitted to the
    /// event log. No estimation happens here: this only renders its geometry.
    pub(crate) fn log_body_parts(&self, estimates: &[BodyPartsEstimate]) {
        if !self.toggles.body_parts {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };

        rec.log(BODY_PARTS_PATH, &rerun::Clear::recursive()).ok();
        for estimate in estimates {
            let actor = actor_path(&estimate.actor_ref);
            for part in &estimate.parts {
                let Some((points, radius)) = geometry_strip(&part.geometry) else {
                    continue;
                };
                let [r, g, b] = body_part_color(part.part);
                let entity = format!("{actor}/{}", part.part.as_str());
                let strip = rerun::LineStrips2D::new([points])
                    .with_colors([rerun::Color::from_unmultiplied_rgba(
                        r,
                        g,
                        b,
                        body_part_alpha(part.stale),
                    )])
                    .with_radii([rerun::Radius::new_scene_units(radius)]);
                if let Err(e) = rec.log(entity.as_str(), &strip) {
                    log::warn!("viz body part {entity} failed: {e}");
                }
                if let Some(depth) = &part.depth {
                    log_optional_scalar(
                        self,
                        rec,
                        &format!("{entity}/depth/median_m"),
                        depth.median_depth_m,
                    );
                    log_optional_scalar(
                        self,
                        rec,
                        &format!("{entity}/depth/relative_to_torso_m"),
                        depth.relative_to_torso_m,
                    );
                    log_optional_scalar(
                        self,
                        rec,
                        &format!("{entity}/depth/valid_ratio"),
                        depth.valid_ratio,
                    );
                    for evidence in &depth.surface_evidence {
                        let base =
                            format!("{entity}/surface/{}/{}", evidence.surface, evidence.zone);
                        log_optional_scalar(
                            self,
                            rec,
                            &format!("{base}/observed_median"),
                            evidence.observed_median,
                        );
                        log_optional_scalar(
                            self,
                            rec,
                            &format!("{base}/residual"),
                            evidence.residual,
                        );
                        self.log_scalar_inner(
                            rec,
                            &format!("{base}/in_envelope"),
                            f64::from(u8::from(evidence.in_envelope)),
                        );
                    }
                }
            }
        }
    }
}

fn log_optional_scalar(
    bridge: &VizBridge,
    rec: &rerun::RecordingStream,
    path: &str,
    value: Option<f32>,
) {
    if let Some(value) = value {
        bridge.log_scalar_inner(rec, path, f64::from(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_geometries_are_closed_but_limb_polylines_are_not() {
        let (bbox, _) = geometry_strip(&BodyGeometry::Bbox([1.0, 2.0, 3.0, 4.0])).unwrap();
        assert_eq!(bbox.first(), bbox.last());

        let (polygon, _) = geometry_strip(&BodyGeometry::Polygon(vec![
            [1.0, 2.0],
            [3.0, 2.0],
            [2.0, 4.0],
        ]))
        .unwrap();
        assert_eq!(polygon.first(), polygon.last());

        let (polyline, _) = geometry_strip(&BodyGeometry::Polyline {
            points: vec![[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]],
            radius: 4.0,
        })
        .unwrap();
        assert_ne!(polyline.first(), polyline.last());
    }

    #[test]
    fn temporal_parts_are_rendered_with_reduced_alpha() {
        assert_eq!(body_part_alpha(false), 235);
        assert_eq!(body_part_alpha(true), 140);
    }
}
