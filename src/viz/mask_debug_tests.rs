
use super::*;
use crate::detection::DetectionMask;
use mana_geometry::compact_mask::CompactMask;
use std::sync::Arc;

fn detection_with_mask(
    compact: CompactMask,
    origin: [u32; 2],
    mask_dims: [u32; 2],
    polygons: Vec<Vec<[f32; 2]>>,
) -> Detection {
    Detection {
        class: "person".into(),
        confidence: 0.9,
        bbox: [0.0, 0.0, 1.0, 1.0],
        keypoints: None,
        mask: Some(DetectionMask {
            compact: Arc::new(compact),
            polygons: Arc::new(polygons),
            origin,
            mask_dims,
        }),
    }
}

#[test]
fn polygon_strip_is_closed_and_in_frame_pixels() {
    let poly = vec![[0.25, 0.1], [0.75, 0.5], [0.25, 0.9]];
    let strip = frame_strip(&poly, 1920.0, 1080.0);
    assert_eq!(
        strip,
        vec![
            [480.0, 108.0],
            [1440.0, 540.0],
            [480.0, 972.0],
            [480.0, 108.0]
        ]
    );
    assert_eq!(strip.first(), strip.last(), "loop closed");
}

#[test]
fn overlay_paints_mask_then_polygon_on_top() {
    // 8x8 block at offset (2,2) inside a 10x10 mask space; frame == mask
    // space, so the frame-normalized polygon (3,4)-(5,4)-(5,6)-(3,6)
    // maps back to those same pixels.
    let compact = CompactMask::from_dense(&[1u8; 64], 8, 8, (2, 2), (10, 10)).unwrap();
    let poly: Vec<[f32; 2]> = vec![[0.3, 0.4], [0.5, 0.4], [0.5, 0.6], [0.3, 0.6]];
    let det = detection_with_mask(compact, [0, 0], [10, 10], vec![poly]);
    let masked: Vec<&Detection> = vec![&det];

    let overlay = build_mask_overlay(&masked, 10, 10, 10, 10);

    assert_eq!(overlay[0], 0, "background stays empty");
    assert_eq!(overlay[8 * 10 + 8], 1, "mask fill inside the block");
    assert_eq!(
        overlay[4 * 10 + 4],
        0xFF,
        "polygon contour drawn over the mask fill"
    );
    assert_eq!(
        overlay[5 * 10 + 6],
        0xFF,
        "polygon contour drawn over the mask fill"
    );
}

#[test]
fn mask_and_polygon_render_in_mask_space() {
    // 8x8 block at offset (2,2) inside a 10x10 mask space.
    let compact = CompactMask::from_dense(&[1u8; 64], 8, 8, (2, 2), (10, 10)).unwrap();
    // Frame == mask space (origin 0, frame 10x10), so polygons are
    // normalized 0..1 over the same coordinates. The contour (3,4)-(5,4)-
    // (5,6)-(3,6) sits inside the block, with room for fill pixels away
    // from the thick border.
    let poly: Vec<[f32; 2]> = vec![[0.3, 0.4], [0.5, 0.4], [0.5, 0.6], [0.3, 0.6]];
    let det = detection_with_mask(compact, [0, 0], [10, 10], vec![poly]);
    let masked: Vec<&Detection> = vec![&det];

    let (mask_img, poly_img, w, h) = render_mask_debug_images(&masked, 10, 10).unwrap();
    assert_eq!((w, h), (10, 10));

    let color = Rgb(PALETTE[0]);
    assert_eq!(
        mask_img.get_pixel(8, 8),
        &color,
        "mask fill away from the border"
    );
    assert_eq!(
        mask_img.get_pixel(0, 0),
        &Rgb([0, 0, 0]),
        "background stays black"
    );

    assert_eq!(
        poly_img.get_pixel(0, 0),
        &Rgb([0, 0, 0]),
        "polygon image starts black"
    );
    let mut line_hit = false;
    for px in 3..=5 {
        if *poly_img.get_pixel(px, 4) == color {
            line_hit = true;
        }
    }
    assert!(line_hit, "polygon edge drawn between (3,4) and (5,4)");

    // The polygon border is drawn over the mask in white and is thick
    // enough to be visible on top of the colored fill.
    assert_eq!(
        mask_img.get_pixel(4, 4),
        &Rgb([255, 255, 255]),
        "white polygon border over the mask fill"
    );
    assert_eq!(
        poly_img.get_pixel(4, 3),
        &color,
        "thick stroke also paints the row above the edge"
    );
}

#[test]
fn polygon_maps_back_from_frame_to_mask_space() {
    // Mask space 4x4 placed at origin (2,3) inside a 10x10 frame.
    let compact = CompactMask::from_dense(&[1, 1, 1, 1], 2, 2, (1, 1), (4, 4)).unwrap();
    // A mask-space point (0.25, 0.25) is frame-normalized as
    // (0.25*4 + 2)/10 = 0.3, (0.25*4 + 3)/10 = 0.4 (as `run()` leaves it).
    let poly: Vec<[f32; 2]> = vec![[0.3, 0.4]];
    let det = detection_with_mask(compact, [2, 3], [4, 4], vec![poly]);
    let masked: Vec<&Detection> = vec![&det];

    let (mask_img, poly_img, w, h) = render_mask_debug_images(&masked, 10, 10).unwrap();
    assert_eq!((w, h), (4, 4));
    assert_eq!(
        mask_img.get_pixel(1, 1),
        &Rgb(PALETTE[0]),
        "mask at (1,1) of mask space"
    );

    // Single-vertex polygon draws no line (len < 2) but the inverse
    // mapping itself is exercised without panicking.
    assert_eq!(poly_img.get_pixel(0, 0), &Rgb([0, 0, 0]));
}

#[test]
fn depth_context_bbox_is_translated_and_clipped_to_roi() {
    let roi = CropRect {
        x1: 560,
        y1: 140,
        x2: 1240,
        y2: 820,
    };

    assert_eq!(
        bbox_in_roi([500.0, 100.0, 700.0, 300.0], roi),
        Some([0.0, 0.0, 140.0, 160.0])
    );
}

#[test]
fn depth_context_bbox_is_ignored_when_outside_roi() {
    let roi = CropRect {
        x1: 560,
        y1: 140,
        x2: 1240,
        y2: 820,
    };

    assert_eq!(bbox_in_roi([0.0, 0.0, 100.0, 100.0], roi), None);
}

#[test]
fn pose_keypoint_visibility_filters_confidence_and_non_finite_points() {
    assert!(pose_keypoint_visible(10.0, 20.0, 0.25));
    assert!(!pose_keypoint_visible(10.0, 20.0, 0.24));
    assert!(!pose_keypoint_visible(f32::NAN, 20.0, 0.9));
}
