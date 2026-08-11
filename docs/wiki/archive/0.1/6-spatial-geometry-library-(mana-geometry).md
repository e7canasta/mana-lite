# Spatial Geometry Library (mana-geometry)

Relevant source files

- [](std/mana-geometry/src/bbox.rs)
- [](std/mana-geometry/src/compact_mask.rs)
- [](std/mana-geometry/src/iou.rs)
- [](std/mana-geometry/src/lib.rs)
- [](std/mana-geometry/src/polygon.rs)
- [](std/mana-geometry/src/polygonize.rs)
- [](std/mana-geometry/src/transform.rs)

The `mana-geometry` crate serves as the central spatial algebra library for the `mana-lite` workspace. It provides a set of primitives and algorithms for handling bounding boxes, pixel-level masks, and vector polygons. The library is designed to be high-performance, often operating in normalized coordinate space ($[0, 1]$) to maintain consistency across different video resolutions and model input sizes.

### System Architecture and Code Mapping

The library bridges the gap between raw inference outputs (rasters and coordinate lists) and high-level spatial reasoning used by the tracker and FSM.

**Spatial Entity Mapping**

**Sources:** [std/mana-geometry/src/lib.rs1-20](std/mana-geometry/src/lib.rs#L1-L20)

---

### Bounding Boxes and IoU

Bounding boxes are the primary spatial representation for object detection and tracking. The library supports multiple formats, including center-format (`cx, cy, w, h`) and corner-format (`x1, y1, x2, y2`).

- **Coordinate Conversions:** Functions like `center_to_corners` [std/mana-geometry/src/bbox.rs8-10](std/mana-geometry/src/bbox.rs#L8-L10) and `corners_to_center` [std/mana-geometry/src/bbox.rs14-16](std/mana-geometry/src/bbox.rs#L14-L16) facilitate transformations between internal logic and external visualization tools.
- **Overlap Metrics:** The library implements Intersection over Union (IoU) and Intersection over Substrate (IoS) via `box_overlap` [std/mana-geometry/src/iou.rs18-38](std/mana-geometry/src/iou.rs#L18-L38) These metrics are critical for the Hungarian algorithm during track assignment and for gating detection cascades.
- **Kalman Compatibility:** Specific converters like `xyxy_to_xcycarh` [std/mana-geometry/src/bbox.rs111-118](std/mana-geometry/src/bbox.rs#L111-L118) provide the aspect-ratio and height format required by the SORT/DeepSORT Kalman filter motion models.

For implementation details on box algebra and assignment gating, see **[Bounding Boxes and IoU](https://deepwiki.com/ernestovisiona-netizen/kik8/6.1-bounding-boxes-and-iou)**.

**Sources:** [std/mana-geometry/src/bbox.rs1-145](std/mana-geometry/src/bbox.rs#L1-L145) [std/mana-geometry/src/iou.rs1-97](std/mana-geometry/src/iou.rs#L1-L97)

---

### Masks and Polygons

For pixel-perfect spatial reasoning, `mana-geometry` provides tools to handle segmentation masks and their vector counterparts.

- **CompactMask (Crop-RLE):** To avoid the memory overhead of full-frame bitmasks, the `CompactMask` struct [std/mana-geometry/src/compact_mask.rs65-69](std/mana-geometry/src/compact_mask.rs#L65-L69) stores masks as column-major Run-Length Encoded (RLE) data scoped strictly to the object's bounding box.
- **Polygonization:** The library can convert raster masks into simplified vector contours using the Suzuki-Abe border following algorithm and Ramer-Douglas-Peucker (RDP) simplification [std/mana-geometry/src/polygonize.rs37-45](std/mana-geometry/src/polygonize.rs#L37-L45)
- **Geometric Algebra:** The `Polygon` type supports area calculation via the Shoelace formula and point-in-polygon tests in `polygon.rs`.

For details on RLE storage and contour extraction, see **[Masks and Polygons](https://deepwiki.com/ernestovisiona-netizen/kik8/6.2-masks-and-polygons)**.

**Sources:** [std/mana-geometry/src/compact_mask.rs1-113](std/mana-geometry/src/compact_mask.rs#L1-L113) [std/mana-geometry/src/polygonize.rs1-122](std/mana-geometry/src/polygonize.rs#L1-L122) [std/mana-geometry/src/polygon.rs1-84](std/mana-geometry/src/polygon.rs#L1-L84)

---

### Coordinate Transforms

The library manages the mapping between different coordinate systems, particularly when dealing with "letterboxed" model inputs where an image is padded to fit a square inference size.

**Transform Workflow**

- **Unletterboxing:** The `Transform` struct [std/mana-geometry/src/transform.rs23-30](std/mana-geometry/src/transform.rs#L23-L30) carries scale factors and padding offsets to reverse preprocessing effects.
- **Proto-Crops:** For segmentation models, `proto_crop_region` [std/mana-geometry/src/transform.rs103-110](std/mana-geometry/src/transform.rs#L103-L110) calculates the exact sub-region of a prototype mask that corresponds to the actual image content, excluding padding.

**Sources:** [std/mana-geometry/src/transform.rs1-133](std/mana-geometry/src/transform.rs#L1-L133)

