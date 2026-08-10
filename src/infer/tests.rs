use super::*;
use crate::config::ModelTask;

#[test]
fn depth_is_extracted_without_detection_boxes() {
    let mut result = Results::new(
        ndarray::Array3::zeros((2, 2, 3)),
        "frame".into(),
        Arc::new(HashMap::new()),
        ultralytics_inference::Speed::default(),
        (2, 2),
    );
    result.depth = Some(DepthMap::new(
        ndarray::array![[1.0, 2.0], [0.0, 3.0]],
        (2, 2),
    ));
    assert!(result.boxes.is_none());

    let depth = take_depth(std::slice::from_mut(&mut result)).expect("depth map");
    assert_eq!(depth.data.shape(), &[2, 2]);
    assert_eq!(depth.min_depth(), Some(1.0));
    assert_eq!(depth.max_depth(), Some(3.0));
    assert!(result.depth.is_none());
}

#[test]
fn enabled_missing_model_fails_catalog_load() {
    let entry = ModelEntry {
        path: std::path::PathBuf::from("/definitely/missing/model.onnx"),
        task: ModelTask::Depth,
        enabled: true,
        confidence: 0.0,
        iou: 0.5,
        max_det: 300,
        imgsz: Some(320),
        device: "cpu".into(),
        half: true,
        rect: true,
        polygon_simplify: 0.98,
        postprocess: PostprocessConfig::default(),
        crop: None,
    };
    let catalog = ModelCatalog {
        models: HashMap::from([(String::from("depth-standard"), entry)]),
    };

    let error = match InferEngine::from_catalog(&catalog) {
        Ok(_) => panic!("enabled missing model must fail startup"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("file not found"));
}

#[test]
fn crop_frame_extracts_static_roi_in_local_pixel_order() {
    let rgb = (0u8..36).collect::<Vec<_>>();
    let crop = extract_crop_frame(
        &rgb,
        4,
        3,
        CropRect {
            x1: 1,
            y1: 1,
            x2: 3,
            y2: 3,
        },
    )
    .expect("valid crop");

    assert_eq!((crop.w, crop.h), (2, 2));
    assert_eq!(
        crop.rgb,
        vec![15, 16, 17, 18, 19, 20, 27, 28, 29, 30, 31, 32]
    );
}

#[test]
fn crop_frame_rejects_roi_outside_frame() {
    assert!(
        extract_crop_frame(
            &[0; 12],
            2,
            2,
            CropRect {
                x1: 1,
                y1: 1,
                x2: 3,
                y2: 2,
            },
        )
        .is_none()
    );
}

fn person(x1: f32, y1: f32, x2: f32, y2: f32) -> Detection {
    Detection {
        class: "person".into(),
        confidence: 0.9,
        bbox: [x1, y1, x2, y2],
        keypoints: None,
        mask: None,
    }
}

fn solid_mask(
    w: usize,
    h: usize,
    x1: usize,
    y1: usize,
    x2: usize,
    y2: usize,
) -> ndarray::Array2<f32> {
    let mut data = ndarray::Array2::<f32>::zeros((h, w));
    for row in y1..y2 {
        for col in x1..x2 {
            data[[row, col]] = 1.0;
        }
    }
    data
}

#[test]
fn mask_builder_produces_compact_rle_and_polygons() {
    let mask = solid_mask(8, 8, 2, 2, 6, 6);
    let built =
        build_detection_mask(mask.view(), [2.0, 2.0, 6.0, 6.0], 8, 8, 0.75, 0.0, 0.5).unwrap();
    assert_eq!(built.compact.offsets, vec![(2, 2)]);
    assert_eq!(built.compact.image_shape, (8, 8));
    assert_eq!(built.compact.area(0).unwrap(), 16);
    assert_eq!(built.compact.rles[0].h, 4);
    assert_eq!(built.compact.rles[0].w, 4);
    assert!(!built.compact.rles[0].counts.is_empty());
    assert!(!built.polygons.is_empty(), "polygons must be derived");
    let rect = &built.polygons[0];
    let shoelace = (0..rect.len())
        .map(|i| {
            let [x1, y1] = rect[i];
            let [x2, y2] = rect[(i + 1) % rect.len()];
            x1 * y2 - x2 * y1
        })
        .sum::<f32>()
        .abs();
    assert!(shoelace > 0.0, "polygon must enclose area, got {shoelace}");
}

#[test]
fn mask_builder_removes_small_components_from_compact_and_polygons() {
    let mut mask = ndarray::Array2::<f32>::zeros((20, 20));
    for row in 2..10 {
        for col in 2..10 {
            mask[[row, col]] = 1.0;
        }
    }
    mask[[17, 17]] = 1.0;

    let built =
        build_detection_mask(mask.view(), [0.0, 0.0, 20.0, 20.0], 20, 20, 0.75, 0.02, 0.5).unwrap();

    assert_eq!(built.compact.area(0).unwrap(), 64);
    assert_eq!(built.polygons.len(), 1);
}

#[test]
fn mask_builder_keeps_multiple_large_components() {
    let mut mask = ndarray::Array2::<f32>::zeros((20, 20));
    for row in 2..8 {
        for col in 2..8 {
            mask[[row, col]] = 1.0;
        }
    }
    for row in 12..18 {
        for col in 12..18 {
            mask[[row, col]] = 1.0;
        }
    }

    let built =
        build_detection_mask(mask.view(), [0.0, 0.0, 20.0, 20.0], 20, 20, 0.75, 0.02, 0.5).unwrap();

    assert_eq!(built.compact.area(0).unwrap(), 72);
    assert_eq!(built.polygons.len(), 2);
}

#[test]
fn mask_builder_omits_mask_when_all_components_are_too_small() {
    let mut mask = ndarray::Array2::<f32>::zeros((20, 20));
    mask[[10, 10]] = 1.0;

    let built = build_detection_mask(mask.view(), [0.0, 0.0, 20.0, 20.0], 20, 20, 0.75, 0.02, 0.5);

    assert!(built.is_none());
}

#[test]
fn polygon_keeps_bbox_offset_in_mask_space() {
    // Solid 4x4 block at (2,2) inside an 8x8 mask: the bbox crop starts
    // at (2,2), so mask_to_polygons' crop-normalized vertices (0..0.75)
    // must land in mask space scaled by 0.5 plus normalized offset 0.25,
    // i.e. exactly [(0.25,0.25)..(0.625,0.625)]. Before the fix the
    // polygon was anchored at the mask-space origin, missing the offset
    // entirely (the margin between crop origin and bbox).
    let mask = solid_mask(8, 8, 2, 2, 6, 6);
    let built =
        build_detection_mask(mask.view(), [2.0, 2.0, 6.0, 6.0], 8, 8, 0.75, 0.0, 0.5).unwrap();
    let rect = &built.polygons[0];
    let min_x = rect.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
    let min_y = rect.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    let max_x = rect.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
    let max_y = rect.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
    assert!(
        (min_x - 0.25).abs() < 0.05 && (min_y - 0.25).abs() < 0.05,
        "polygon must start at the bbox offset, got min ({min_x}, {min_y})"
    );
    assert!(
        (max_x - 0.625).abs() < 0.05 && (max_y - 0.625).abs() < 0.05,
        "polygon must end at the bbox extent, got max ({max_x}, {max_y})"
    );
    assert!(
        (max_x - min_x) < 0.45,
        "polygon scale must stay in range, got {}",
        max_x - min_x
    );
}

#[test]
fn mask_builder_ignores_below_threshold_pixels() {
    let mut mask = ndarray::Array2::<f32>::zeros((8, 8));
    mask[[4, 4]] = 0.4;
    let built = build_detection_mask(mask.view(), [3.0, 3.0, 5.0, 5.0], 8, 8, 0.75, 0.0, 0.5);
    assert!(built.is_none(), "no pixel above threshold -> no mask");
}

#[test]
fn mask_builder_bbox_outside_mask_is_none() {
    let mask = solid_mask(8, 8, 2, 2, 6, 6);
    let built = build_detection_mask(mask.view(), [9.0, 9.0, 10.0, 10.0], 8, 8, 0.75, 0.0, 0.5);
    assert!(built.is_none());
}

#[test]
fn mask_builder_respects_partial_bbox_clip() {
    let mask = solid_mask(8, 8, 0, 0, 8, 8);
    let built =
        build_detection_mask(mask.view(), [-2.0, -2.0, 4.0, 4.0], 8, 8, 0.75, 0.0, 0.5).unwrap();
    assert_eq!(built.compact.area(0).unwrap(), 16, "clipped 4x4 block");
    assert_eq!(built.compact.offsets, vec![(0, 0)]);
}

#[test]
fn roi_largest_class_simple() {
    let dets = vec![person(100.0, 100.0, 200.0, 300.0)];
    let r = compute_largest_class_roi(&dets, "person", 0.0, 640, 480, None, None).unwrap();
    assert_eq!(
        r,
        CropRect {
            x1: 100,
            y1: 100,
            x2: 200,
            y2: 300
        }
    );
}

#[test]
fn roi_min_region_union() {
    let dets = vec![person(300.0, 100.0, 400.0, 200.0)];
    let r = compute_largest_class_roi(
        &dets,
        "person",
        0.0,
        640,
        480,
        Some([100, 200, 500, 450]),
        None,
    )
    .unwrap();
    assert_eq!(
        r,
        CropRect {
            x1: 100,
            y1: 100,
            x2: 500,
            y2: 450
        }
    );
}

#[test]
fn roi_min_region_fallback() {
    let dets: Vec<Detection> = vec![];
    let r = compute_largest_class_roi(
        &dets,
        "person",
        0.0,
        640,
        480,
        Some([100, 200, 500, 450]),
        None,
    )
    .unwrap();
    assert_eq!(
        r,
        CropRect {
            x1: 100,
            y1: 200,
            x2: 500,
            y2: 450
        }
    );
}

#[test]
fn roi_max_region_clamps() {
    let dets = vec![person(0.0, 0.0, 640.0, 480.0)];
    let r = compute_largest_class_roi(
        &dets,
        "person",
        0.0,
        640,
        480,
        None,
        Some([50, 50, 400, 300]),
    )
    .unwrap();
    assert_eq!(
        r,
        CropRect {
            x1: 50,
            y1: 50,
            x2: 400,
            y2: 300
        }
    );
}

#[test]
fn roi_min_max_together() {
    let dets = vec![person(200.0, 100.0, 300.0, 200.0)];
    let r = compute_largest_class_roi(
        &dets,
        "person",
        0.0,
        640,
        480,
        Some([50, 50, 500, 400]),
        Some([0, 0, 350, 300]),
    )
    .unwrap();
    assert_eq!(
        r,
        CropRect {
            x1: 50,
            y1: 50,
            x2: 350,
            y2: 300
        }
    );
}

#[test]
fn roi_no_class_no_min() {
    let dets: Vec<Detection> = vec![];
    let r = compute_largest_class_roi(&dets, "person", 0.0, 640, 480, None, None);
    assert!(r.is_none());
}

#[test]
fn roi_from_track_bbox_matches_dynamic_roi() {
    let r = compute_bbox_roi([100.0, 100.0, 200.0, 300.0], 0.0, 640, 480, None, None).unwrap();
    assert_eq!(
        r,
        CropRect {
            x1: 100,
            y1: 100,
            x2: 200,
            y2: 300
        }
    );
}

#[test]
fn upper_square_roi_focuses_on_person_upper_half() {
    let r = compute_upper_square_roi([768.0, 190.0, 986.0, 717.0], 320, 0.5, 1920, 1080).unwrap();
    assert_eq!(r.x2 - r.x1, 320);
    assert_eq!(r.y2 - r.y1, 320);
    assert!(r.y1 < 190);
    assert!(r.y2 < 550);
}

#[test]
fn pose_keypoints_follow_dynamic_crop_offset() {
    let mut keypoints = [[10.0, 20.0, 0.9], [30.0, 40.0, 0.8]];
    translate_keypoints(&mut keypoints, 100.0, 50.0);
    assert_eq!(keypoints, [[110.0, 70.0, 0.9], [130.0, 90.0, 0.8]]);
}

#[test]
fn model_postprocess_filter_accepts_only_valid_detections() {
    let filters = PostprocessConfig {
        allow_classes: vec!["person".into()],
        min_confidence: 0.5,
        min_area_ratio: 0.01,
        max_area_ratio: 0.8,
        min_component_area_ratio: 0.0,
        mask_threshold: 0.5,
        nms_iou: 0.5,
        max_detections: None,
    };
    assert!(filters.accepts("person", 0.8, [0.0, 0.0, 20.0, 20.0], 100, 100));
    assert!(!filters.accepts("chair", 0.8, [0.0, 0.0, 20.0, 20.0], 100, 100));
    assert!(!filters.accepts("person", 0.4, [0.0, 0.0, 20.0, 20.0], 100, 100));
    assert!(!filters.accepts("person", 0.8, [0.0, 0.0, 5.0, 5.0], 100, 100));
    assert!(!filters.accepts("person", 0.8, [0.0, 0.0, 100.0, 100.0], 100, 100));
    assert!(!filters.accepts("person", 0.8, [-1.0, 0.0, 20.0, 20.0], 100, 100));
}

#[test]
fn static_roi_clips_boxes_to_roi_bounds() {
    let detection = Detection {
        class: "person".into(),
        confidence: 0.9,
        bbox: [550.0, 130.0, 1250.0, 830.0],
        keypoints: None,
        mask: None,
    };

    let clipped = clip_detection_to_roi(
        detection,
        CropRect {
            x1: 560,
            y1: 140,
            x2: 1240,
            y2: 820,
        },
    )
    .unwrap();
    assert_eq!(clipped.bbox, [560.0, 140.0, 1240.0, 820.0]);
}

#[test]
fn static_roi_discards_boxes_outside_roi() {
    let detection = Detection {
        class: "person".into(),
        confidence: 0.9,
        bbox: [0.0, 0.0, 100.0, 100.0],
        keypoints: None,
        mask: None,
    };

    assert!(
        clip_detection_to_roi(
            detection,
            CropRect {
                x1: 560,
                y1: 140,
                x2: 1240,
                y2: 820
            }
        )
        .is_none()
    );
}

#[test]
fn explicit_nms_suppresses_overlapping_same_class_only() {
    let detections = vec![
        Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [0.0, 0.0, 100.0, 100.0],
            keypoints: None,
            mask: None,
        },
        Detection {
            class: "person".into(),
            confidence: 0.8,
            bbox: [10.0, 10.0, 90.0, 90.0],
            keypoints: None,
            mask: None,
        },
        Detection {
            class: "wheelchair".into(),
            confidence: 0.7,
            bbox: [10.0, 10.0, 90.0, 90.0],
            keypoints: None,
            mask: None,
        },
    ];
    let (kept, suppressed) = apply_nms(detections, 0.5);
    assert_eq!(suppressed, 1);
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].class, "person");
    assert_eq!(kept[1].class, "wheelchair");
}

#[test]
fn low_face_nms_iou_keeps_highest_confidence_overlap() {
    let detections = vec![
        Detection {
            class: "face".into(),
            confidence: 0.91,
            bbox: [0.0, 0.0, 100.0, 100.0],
            keypoints: None,
            mask: None,
        },
        Detection {
            class: "face".into(),
            confidence: 0.72,
            bbox: [80.0, 0.0, 180.0, 100.0],
            keypoints: None,
            mask: None,
        },
    ];
    let (kept, suppressed) = apply_nms(detections, 0.05);
    assert_eq!(suppressed, 1);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].confidence, 0.91);
}

#[test]
fn max_detections_keeps_top_confidence_after_nms() {
    let detections = vec![
        Detection {
            class: "face".into(),
            confidence: 0.91,
            bbox: [0.0, 0.0, 50.0, 50.0],
            keypoints: None,
            mask: None,
        },
        Detection {
            class: "face".into(),
            confidence: 0.72,
            bbox: [100.0, 0.0, 150.0, 50.0],
            keypoints: None,
            mask: None,
        },
    ];
    let (mut kept, nms_suppressed) = apply_nms(detections, 0.05);
    let removed = apply_max_detections(&mut kept, Some(1));
    assert_eq!(nms_suppressed + removed, 1);
    assert_eq!(kept[0].confidence, 0.91);
}
