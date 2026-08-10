use rerun::datatypes::Vec2D;

/// Build a Boxes2D archetype from an axis-aligned xyxy pixel box.
pub(super) fn boxes2d_from_xyxy(
    bbox: [f32; 4],
    color: rerun::Color,
    label: Option<&str>,
    radius: f32,
) -> rerun::Boxes2D {
    let [x1, y1, x2, y2] = bbox;
    let boxes = rerun::Boxes2D::from_centers_and_half_sizes(
        [Vec2D([(x1 + x2) / 2.0, (y1 + y2) / 2.0])],
        [Vec2D([(x2 - x1).abs() / 2.0, (y2 - y1).abs() / 2.0])],
    )
    .with_colors([color])
    .with_radii([radius]);
    match label {
        Some(label) => boxes.with_labels([label]),
        None => boxes,
    }
}
