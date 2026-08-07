Here is the complete data flow analysis, tracing crop configuration all the way through tensor slicing and Rerun logging.

---

## 1. How crop/ROI is applied in the inference pipeline

### Type definition

**File:** `/home/care/opt/workspace/references/mana-lite/src/infer.rs`, line 11

```rust
pub type CropRect = (u32, u32, u32, u32);
```

`CropRect` is simply a type alias for a 4-tuple of `u32`: `(x1, y1, x2, y2)` representing pixel coordinates on the full frame.

### Config parsing

**File:** `/home/care/opt/workspace/references/mana-lite/src/config.rs`, lines 220–257

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct CropConfig {
    #[serde(rename = "type")]
    pub crop_type: CropType,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default = "default_crop_margin")]
    pub margin: f32,
    #[serde(default)]
    pub region: Option<[u32; 4]>,     // ← static crop: [x1,y1,x2,y2]
    #[serde(default)]
    pub min_region: Option<[u32; 4]>,
    #[serde(default)]
    pub max_region: Option<[u32; 4]>,
    #[serde(default)]
    pub fallback: FallbackMode,
}
```

`CropType` is an enum with two variants (`Static`, `LargestClass`). The `region` field stores the static crop as `[u32; 4]`.

Each `ModelEntry` (line 199) holds an optional `CropConfig`:

```rust
pub crop: Option<CropConfig>,
```

### Model loading — static crop forwarded to ONNX runtime via ROI

**File:** `/home/care/opt/workspace/references/mana-lite/src/infer.rs`, lines 48–76

During `InferEngine::from_catalog()`, if a model has a `Static` crop with a `region`, the coordinates are passed into the `ultralytics_inference` engine as an ONNX-level ROI *preprocessing* hint:

```rust
if let Some(ref crop) = entry.crop {
    if crop.crop_type == CropType::Static {
        if let Some([x1, y1, x2, y2]) = crop.region {
            conf = conf.with_roi(x1, y1, x2, y2);  // line 53
        }
    }
}
```

This is a **separate ROI path** — it tells the ONNX runtime to crop as a preprocessing step. However, the model is still stored with `crop_config: None` in that case (line 65–68):

```rust
crop_config: entry.crop.as_ref()
    .filter(|c| c.crop_type == CropType::LargestClass)
    .cloned(),
```

So `crop_info()` only returns `Some(...)` for `LargestClass` crops. For `Static` crops, the ROI is delegated entirely to the ultralytics inference layer via `with_roi()`, and the mana-lite layer does NOT do any additional cropping.

### Runtime crop resolution — where the rect is computed

**File:** `/home/care/opt/workspace/references/mana-lite/src/main.rs`, lines 280–298

```rust
fn resolve_crop_rect(
    &self,
    model_key: &str,
    model_dets: &HashMap<String, Vec<Detection>>,
    fb: &FrameBuffer,
) -> Option<infer::CropRect> {
    let crop_cfg = self.infer.crop_info(model_key)?;

    if crop_cfg.crop_type == CropType::Static {
        return crop_cfg.region.map(|[x1, y1, x2, y2]| (x1, y1, x2, y2));
    }

    let class = crop_cfg.class.as_ref()?;
    let parent_dets = self.cascade.parent_of(model_key)
        .and_then(|pk| model_dets.get(pk));

    let dets_slice = parent_dets.map(|v| v.as_slice()).unwrap_or(&[]);
    compute_largest_class_roi(dets_slice, class, crop_cfg.margin, fb.w, fb.h,
                              crop_cfg.min_region, crop_cfg.max_region)
}
```

Two paths:

- **Static:** directly returns `crop_cfg.region` as `(x1,y1,x2,y2)`
- **LargestClass:** calls `compute_largest_class_roi()` using parent model detections

### `compute_largest_class_roi` — the dynamic ROI math

**File:** `/home/care/opt/workspace/references/mana-lite/src/infer.rs`, lines 189–241

This function finds the largest bbox matching a target class, expands by `margin`, clamps to frame boundaries, then unions with `min_region` and clamps to `max_region`.

### The actual tensor slicing — where the frame is cropped

**File:** `/home/care/opt/workspace/references/mana-lite/src/infer.rs`, lines 88–130 (the `InferEngine::run()` method)

```rust
pub fn run(
    &mut self,
    model_key: &str,
    rgb: &[u8],
    w: u32,
    h: u32,
    crop_rect: Option<CropRect>,
) -> Option<(Vec<Detection>, u64)> {
    let loaded = self.models.get_mut(model_key)?;

    let (img, offset_x, offset_y) = if let Some((x1, y1, x2, y2)) = crop_rect {
        let crop_w = x2 - x1;
        let crop_h = y2 - y1;
        if crop_w == 0 || crop_h == 0 {
            return None;
        }
        let mut cropped = vec![0u8; (crop_w * crop_h * 3) as usize];
        for row in y1..y2 {
            let src_off = (row * w + x1) as usize * 3;
            let dst_off = ((row - y1) * crop_w) as usize * 3;
            cropped[dst_off..dst_off + (crop_w as usize * 3)]
                .copy_from_slice(&rgb[src_off..src_off + (crop_w as usize * 3)]);
        }
        let img = DynamicImage::ImageRgb8(RgbImage::from_raw(crop_w, crop_h, cropped)?);
        (img, x1 as f32, y1 as f32)
    } else {
        let img = DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, rgb.to_vec())?);
        (img, 0.0, 0.0)
    };
    // ... runs inference, then adjusts bboxes back to full-frame coords:
    for d in &mut detections {
        d.bbox[0] += offset_x;  // back to full-frame
        d.bbox[1] += offset_y;
        d.bbox[2] += offset_x;
        d.bbox[3] += offset_y;
    }
```

The cropping is a manual row-by-row copy from the full `rgb` buffer into a new `cropped` buffer, creating a `DynamicImage`. The offset `(x1, y1)` is stored so that all detection bboxes are corrected back to full-frame pixel coordinates after inference (lines 121–126).

### How `run_inference` orchestrates all of this

**File:** `/home/care/opt/workspace/references/mana-lite/src/main.rs`, lines 254–277

```rust
fn run_inference(&mut self, fb: &FrameBuffer, config: &AppConfig) {
    // ...
    for model_key in &ordered {
        let crop_rect = self.resolve_crop_rect(model_key, &model_dets, fb);
        // ...
        if let Some((detections, infer_ms)) = self.infer.run(model_key, &fb.rgb, fb.w, fb.h, crop_rect) {
            // ...
        }
    }
}
```

The loop iterates cascade-ordered models. Each model gets a crop rect resolved (from static config or dynamic largest-class logic), then `infer.run()` applies the actual crop, runs the model, and offsets bboxes back.

---

## 2. How frames are sent to Rerun when `frames = true`

### The toggle check and call site

**File:** `/home/care/opt/workspace/references/mana-lite/src/main.rs`, lines 376–383

```rust
fn flush_viz_metrics(&mut self, frame_buf: &Option<FrameBuffer>) {
    if let Some(fb) = frame_buf.as_ref() {
        self.viz.log_frame(
            &raw_frame_header(fb, self.state.frame_number()),
            &fb.rgb,
        );
    }
}
```

This is called at the end of `process_keyframe()` (line 245). It sends the **full decoded frame** (the entire `fb.rgb` buffer) — there is no cropping applied here.

### The `log_frame` method on `VizBridge`

**File:** `/home/care/opt/workspace/references/mana-lite/src/viz.rs`, lines 166–173

```rust
pub fn log_frame(&self, header: &RawFrameV1, rgb: &[u8]) {
    if !self.toggles.frames { return; }           // guard: skips if `frames = false`
    if let Inner::Connected { ref rec, .. } = self.inner {
        if let Err(e) = logging::frame::log_frame_rgb24(rec, "/world/camera/bgr", header, rgb) {
            log::warn!("viz frame log failed: {e}");
        }
    }
}
```

The entity path is hardcoded: `"/world/camera/bgr"`.

### The underlying Rerun logging function

**File:** `/home/care/opt/workspace/references/mana-lite/std/mana-viz/src/logging/frame.rs`, lines 6–44

```rust
pub fn log_frame_rgb24(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    header: &RawFrameV1,
    rgb: &[u8],
) -> Result<()> {
    // validates/truncates/pads to (w*h*3) bytes
    log_frame_rgb24_owned(rec, entity_path, header, data)
}

pub fn log_frame_rgb24_owned(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    header: &RawFrameV1,
    data: Vec<u8>,
) -> Result<()> {
    log_at(
        rec, entity_path, header.timestamp_ns,
        &rerun::Image::from_rgb24(data, [header.width, header.height]),
        || format!("rrd frame log failed (frame_id={})", header.frame_id),
    )
}
```

Which calls `util::log_at` (`/home/care/opt/workspace/references/mana-lite/std/mana-viz/src/logging/util.rs`, lines 38–47) that sets the time sequence and logs the archetype.

**Key takeaway:** The full frame is always sent to Rerun at path `/world/camera/bgr`. The `frames` toggle only enables/disables this send. There is no conditional cropping of what image is sent.

---

## 3. How bounding boxes are sent to Rerun

### Call site

**File:** `/home/care/opt/workspace/references/mana-lite/src/main.rs`, line 317 (inside `record_model_result`)

```rust
self.viz.log_detection_boxes(model_key, detections);
```

### The `log_detection_boxes` method

**File:** `/home/care/opt/workspace/references/mana-lite/src/viz.rs`, lines 175–208

```rust
pub fn log_detection_boxes(&self, model: &str, detections: &[Detection]) {
    if !self.toggles.boxes { return; }
    let rec = match &self.inner {
        Inner::Connected { rec, .. } => rec,
        _ => return,
    };
    let path = format!("/world/camera/detections/{model}");
    rec.log(path.as_str(), &rerun::Clear::recursive()).ok();

    for (i, det) in detections.iter().enumerate() {
        let class = sanitize_entity_name(&det.class);
        let entity = format!("{path}/{class}/{i}");
        let x1 = det.bbox[0]; let y1 = det.bbox[1];
        let x2 = det.bbox[2]; let y2 = det.bbox[3];
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2) / 2.0;
        let hw = (x2 - x1).abs() / 2.0;
        let hh = (y2 - y1).abs() / 2.0;
        // ...creates rerun::Boxes2D with colors/labels...
        rec.log(entity.as_str(), &bbox);
    }
}
```

### Coordinates are correct for the full frame

Yes, the detection bboxes are in full-frame pixel coordinates. As shown in question 1, `infer.rs` lines 121–126 apply the crop offset back to the full frame:

```rust
d.bbox[0] += offset_x;
d.bbox[1] += offset_y;
d.bbox[2] += offset_x;
d.bbox[3] += offset_y;
```

The entity paths for boxes follow the pattern:
`/world/camera/detections/{model}/{class_name}/{index}`

### mana-viz also has an unused `log_detections_2d`

**File:** `/home/care/opt/workspace/references/mana-lite/std/mana-viz/src/logging/boxes.rs`, lines 9–33

This function (`log_detections_2d`) takes `DetectionBatchV1` (normalized coords `[0..1]`) and converts to pixels via `box_halfsize_to_pixels`. However, it is **not called** from mana-lite — the `viz.rs` code does its own direct `rerun::Boxes2D` construction with pixel coordinates.

---

## 4. The Rerun entity tree structure

Based on all the hardcoded paths in `viz.rs` and the blueprint in `rerun.toml`, here is the complete entity tree:

```
/world
  /camera                                    ← Spatial2DView origin
    /bgr                                     ← full RGB image (rerun::Image)
    /detections                              ← (clear recursively each frame)
      /{model_name}                          ← e.g. "detect-fast", "pose-standard"
        /{class_name}                        ← sanitized, e.g. "person", "bed"
          /0, /1, ...                        ← per-detection Boxes2D
                                                    (full-frame pixel coords)

/pipeline
  /decode
    /latency_us                              ← Scalar: decode time per frame
  /infer
    /{model_name}
      /latency_us                            ← Scalar: inference time

/infer                                       ← TimeSeriesView origin
  /{model_safe}                              ← e.g. "detect_fast" (underscores)
    /per_frame
      /counts/{cls_safe}                     ← Scalar: count per frame
      /conf/{cls_safe}/min, /max             ← Scalars
      /area/{cls_safe}/min, /max             ← Scalars

/ingest
  /normal
    /gap_ms                                  ← Scalar: ms between keyframes
```

Blueprint views defined in `/home/care/opt/workspace/references/mana-lite/src/viz.rs` lines 83–128:

| View Name     | Type          | Origin              | Contents                                      |
|---------------|---------------|---------------------|-----------------------------------------------|
| Camera        | Spatial2DView | `/world/camera`     | `+ $origin/**`                                |
| Counts        | TimeSeriesView| `/infer`            | `+ /infer/**/per_frame/counts/**`             |
| Confidence    | TimeSeriesView| `/infer`            | `+ /infer/**/per_frame/conf/**`               |
| Area          | TimeSeriesView| `/infer`            | `+ /infer/**/per_frame/area/**`               |
| Latency       | TimeSeriesView| `/pipeline`         | `+ /pipeline/infer/**/latency_us`, `+ /pipeline/decode/latency_us` |
| Stream        | TimeSeriesView| `/ingest/normal`    | `+ /ingest/normal/gap_ms`                     |

---

## 5. Existing mechanism to show crop/ROI rectangle on the full frame

### In mana-lite: **NONE**

There is **no existing code** that sends a crop window overlay or ROI rectangle to Rerun from mana-lite's own code paths. No function in `viz.rs` or the `logging` module draws a rectangle representing the crop region on the full frame image.

### In the mana-viz crate: **YES**, but unused

**File:** `/home/care/opt/workspace/references/mana-lite/std/mana-viz/src/logging/boxes.rs`, lines 48–118

There is a `log_roi_2d` function that renders ROI rectangles on a 2D view:

```rust
pub fn log_roi_2d(
    rec: &rerun::RecordingStream,
    parent_path: &str,
    cmd: &RoiCommandV1,
    frame: FrameSize,
) -> Result<()> { ... }
```

It supports multiple ROI modes (`FULL`, `CENTER_SQUARE`, `RECT`, `BED`) and draws colored boxes (`roi_fill` = yellow with 60 alpha, line 131–133). However, this function takes `RoiCommandV1` from `mana-types` — a protocol type used by the mana ecosystem — and is **not called anywhere in mana-lite**. The `RoiCommandV1` struct (defined at `/home/care/opt/workspace/references/mana-lite/std/mana-types/src/lib.rs`, lines 290–313) has fields `x, y, w, h` as normalized floats `[0..1]` with a `target_name` string and a `mode` byte.

So the *ability* to draw ROI boxes exists in the shared crate, but nobody in mana-lite invokes it.

---

## 6. `CropRect` fields and static crop coordinates

### Definition

**File:** `/home/care/opt/workspace/references/mana-lite/src/infer.rs`, line 11

```rust
pub type CropRect = (u32, u32, u32, u32);
```

It's `(x1, y1, x2, y2)` — pixel coordinates representing the top-left and bottom-right corners of the crop rectangle, **in full-frame pixel space**.

### Static crop configuration

**File:** `/home/care/opt/workspace/references/mana-lite/src/config.rs`, line 229

```rust
pub region: Option<[u32; 4]>,   // [x1, y1, x2, y2]
```

Deserialized from TOML like:

```toml
[models.detect-fast.crop]
type = "static"
region = [100, 50, 500, 400]
```

### How static crop coordinates flow

When `crop_type == Static`, the `[x1, y1, x2, y2]` from `CropConfig.region` flows through two channels:

1. **At model load time** (infer.rs line 53): sent to the ONNX inference engine via `with_roi(x1, y1, x2, y2)` as a preprocessing hint.

2. **At runtime in `resolve_crop_rect`** (main.rs line 289): returned directly as `(x1, y1, x2, y2)` to the `infer.run()` call, which does the manual row-copy crop in infer.rs lines 92–105.

The `(x1, y1)` values become `offset_x, offset_y`, which are later added to all detection bboxes to convert them back to full-frame pixel coordinates.

### For `LargestClass` crops

The `region` field is ignored. Instead, `compute_largest_class_roi()` (infer.rs lines 189–241) dynamically computes the crop rect from parent detections, using `margin`, `min_region`, and `max_region` from the config.

---

## Summary: Full data flow

```
config.toml / models.toml
    │
    ▼
CropConfig { crop_type, region, margin, ... }
    │
    ├─► [Static] InferEngine::from_catalog()
    │       │  → with_roi() to ONNX engine (preprocessing hint)
    │       │  → crop_config stored as None (handled at ONNX level)
    │       │
    │       ▼  resolve_crop_rect() returns region as (x1,y1,x2,y2)
    │          → InferEngine::run() manually copies rows to cropped buffer
    │          → runs YOLO on cropped image
    │          → adds offset back to bboxes → full-frame pixel coords
    │
    └─► [LargestClass] InferEngine stores CropConfig in LoadedModel
            │  → resolve_crop_rect() calls compute_largest_class_roi()
            │  → returns dynamically computed (x1,y1,x2,y2)
            ▼  same run() flow as above

After inference:
    │
    ├─► VizBridge::log_frame(full_rgb) → "/world/camera/bgr"
    │       (always full frame, never cropped)
    │
    ├─► VizBridge::log_detection_boxes(model, detections)
    │       → "/world/camera/detections/{model}/{class}/{idx}"
    │       Boxes2D in full-frame pixel coords (bboxes already offset-corrected)
    │
    └─► VizBridge::log_per_frame_class_stats(model, per_class)
            → "/infer/{model}/per_frame/..." (scalar timeseries)

NO crop window / ROI rectangle is currently sent to Rerun.
The `log_roi_2d()` function exists in mana-viz but is never called.
```
