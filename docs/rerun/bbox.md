Here is a complete breakdown of how detection boxes are logged to Rerun in the `mana-viz` crate.

---

## 1. Core function: `log_detections_2d`

**File:** `/home/care/opt/workspace/references/mana-lite/std/mana-viz/src/logging/boxes.rs`

### Exact function signature

```rust
pub fn log_detections_2d(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    batch: &DetectionBatchV1,
    frame: FrameSize,
) -> Result<()>
```

### How it works (step by step)

```rust
use anyhow::Result;
use mana_types::bbox;
use mana_types::{DetectionBatchV1, SceneMsgV1, RoiCommandV1};

use rerun::datatypes::Vec2D;

use super::util::{log_archetype, log_many, FrameSize};

pub fn log_detections_2d(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    batch: &DetectionBatchV1,
    frame: FrameSize,
) -> Result<()> {
    let dets = batch.valid();                                          // 1. Get valid slice
    rec.set_time_sequence("frame_ns", batch.timestamp_ns);             // 2. Set timeline
    rec.log(entity_path, &rerun::Clear::recursive()).ok();             // 3. Clear previous frame
    if dets.is_empty() {
        return Ok(());
    }
    let mut class_idx = std::collections::HashMap::with_capacity(4);   // 4. Per-class counter
    let items = dets.iter().map(|d| {
        let e = class_idx.entry(d.class_id).or_insert(0);
        let i = *e;
        *e += 1;
        // 5. Convert normalized coords -> pixel half-sizes
        let (cx, cy, hw, hh) = bbox::box_halfsize_to_pixels(d.cx, d.cy, d.w, d.h, frame.w, frame.h);
        // 6. Build label string
        let label = format!("cls:{} cf:{:.2}", d.class_id, d.confidence);
        // 7. Create a rerun::Boxes2D archetype for this single detection
        let boxes = single_box([cx, cy], [hw, hh], d.class_id, label);
        // 8. Entity path: "/world/camera/detections/{class_id}/{i}"
        (format!("{entity_path}/{}/{}", d.class_id, i), boxes)
    });
    log_many(rec, batch.timestamp_ns, items, "detection");
    Ok(())
}
```

### The helper: `single_box`

```rust
fn single_box(
    center: [f32; 2],
    half_size: [f32; 2],
    class_id: u16,
    label: String,
) -> rerun::Boxes2D {
    rerun::Boxes2D::from_centers_and_half_sizes([Vec2D(center)], [Vec2D(half_size)])
        .with_class_ids([class_id])
        .with_labels([label])
}
```

### The helper: `log_many` (in `util.rs`)

```rust
pub(super) fn log_many<A: rerun::AsComponents>(
    rec: &rerun::RecordingStream,
    timestamp_ns: i64,
    items: impl IntoIterator<Item = (String, A)>,
    what: &str,
) {
    rec.set_time_sequence("frame_ns", timestamp_ns);
    for (path, archetype) in items {
        if let Err(err) = rec.log(path.as_str(), &archetype) {
            tracing::warn!(%err, target_path = %path, "{what} log failed");
        }
    }
}
```

### The coordinate conversion: `box_halfsize_to_pixels`

**File:** `/home/care/opt/workspace/references/mana-lite/std/mana-types/src/lib.rs`, line 324

```rust
pub mod bbox {
    #[inline]
    pub fn box_halfsize_to_pixels(cx: f32, cy: f32, w: f32, h: f32, frame_w: u32, frame_h: u32) -> (f32, f32, f32, f32) {
        let fw = frame_w as f32;
        let fh = frame_h as f32;
        (cx * fw, cy * fh, w * fw / 2.0, h * fh / 2.0)
    }
}
```

This converts normalized `(cx,cy,w,h)` where values are in `[0.0, 1.0]` to pixel-space `(cx, cy, half_w, half_h)`.

---

## 2. Rerun types (archetypes) used

| Rerun Type | Usage | File |
|---|---|---|
| **`rerun::Boxes2D`** | Detections, ROI boxes, zones | `boxes.rs` |
| **`rerun::Image`** | Camera frames (RGB24) | `frame.rs` |
| **`rerun::LineStrips2D`** | BED-mode ROI trapezoid outline | `boxes.rs:102` |
| **`rerun::Scalars::single()`** | Time-series metrics | `viz.rs:167` |
| **`rerun::Clear::recursive()`** | Clear previous-frame detections | `boxes.rs:17` |
| **`rerun::Clear::flat()`** | Clear specific entities | `boxes.rs:89-90` |

Supporting datatypes:

| Rerun datatype | Usage |
|---|---|
| **`rerun::datatypes::Vec2D`** | Constructing 2D points for Boxes2D and LineStrips2D |
| **`rerun::datatypes::Rgba32`** | ROI fill colors (e.g., `from_unmultiplied_rgba`) |

**No** usage of `rerun::Arrows2D`, `rerun::Rect2D`, `rerun::Points2D`, or `rerun::Ellipses2D` was found in this crate.

---

## 3. `DetectionBatchV1` type structure

**File:** `/home/care/opt/workspace/references/mana-lite/std/mana-types/src/lib.rs`, lines 85-127

```rust
pub const MAX_DETECTIONS: usize = 128;

#[derive(Copy, Clone, Debug, Default)]
pub struct DetectionV1 {
    pub cx: f32,            // normalized center x [0.0, 1.0]
    pub cy: f32,            // normalized center y [0.0, 1.0]
    pub w: f32,             // normalized width   [0.0, 1.0]
    pub h: f32,             // normalized height  [0.0, 1.0]
    pub confidence: f32,    // detection confidence score
    pub class_id: u16,      // class label integer
}

#[derive(Clone, Debug)]
pub struct DetectionBatchV1 {
    pub frame_id: u64,
    pub timestamp_ns: i64,
    pub schema_version: u32,
    pub model_fingerprint: [u8; 32],
    pub count: u32,
    pub detections: [DetectionV1; MAX_DETECTIONS],  // fixed 128-slot array
    pub source_id: u64,
}

impl DetectionBatchV1 {
    pub fn valid(&self) -> &[DetectionV1] {
        let count = self.count.min(MAX_DETECTIONS as u32) as usize;
        &self.detections[..count]   // slice of only the valid entries
    }
}
```

### Mapping from `DetectionBatchV1` -> Rerun

| `DetectionBatchV1` field | How it maps to Rerun |
|---|---|
| `timestamp_ns` | Set as Rerun timeline via `rec.set_time_sequence("frame_ns", ...)` |
| `count` + `detections[]` | Iterated via `.valid()`; each `DetectionV1` becomes one `rerun::Boxes2D` entity |
| `DetectionV1.cx, cy, w, h` | Converted via `bbox::box_halfsize_to_pixels()` to pixel-space center + half-sizes for `Boxes2D::from_centers_and_half_sizes` |
| `DetectionV1.class_id` | Set via `.with_class_ids([class_id])` |
| `DetectionV1.confidence` | Formatted into a label string `"cls:{class_id} cf:{confidence:.2}"`, set via `.with_labels([label])` |

**Each detection becomes its own entity path:** `/world/camera/detections/{class_id}/{index}`, so Rerun renders each as an independently selectable box in the 2D space.

---

## 4. How detections land on the camera view (in `mana-lite`)

**File:** `/home/care/opt/workspace/references/mana-lite/src/viz.rs`

### The `VizBridge` struct

```rust
pub struct VizBridge {
    rec: rerun::RecordingStream,
    frame_w: u32,
    frame_h: u32,
    // ...
}
```

### Blueprint: camera view with detection overlay

```rust
fn send_blueprint(rec: &rerun::RecordingStream) {
    let camera_view = rerun::blueprint::Spatial2DView::new("Camera")
        .with_origin("/world/camera")
        .with_contents(["+ $origin/**"]);   // automatically includes detections
    // ...
}
```

This means all entities under `/world/camera/` -- including `/world/camera/detections/{class_id}/{i}` -- are rendered inside the same 2D view as the camera image (which is logged at `/world/camera/bgr`).

### Wiring it to incoming detections

```rust
pub fn log_detections(&self, batch: &DetectionBatchV1) {
    if let Err(e) = logging::boxes::log_detections_2d(
        &self.rec, "/world/camera/detections", batch, self.frame_size(),
    ) {
        log::warn!("viz detection log failed: {e}");
    }
}
```

The entity path prefix is `/world/camera/detections`, so each detection becomes `/world/camera/detections/0/0`, `/world/camera/detections/0/1`, `/world/camera/detections/1/0`, etc.

---

## 5. Complete data-flow code example

```rust
use mana_types::{DetectionBatchV1, DetectionV1, MAX_DETECTIONS};
use mana_viz::logging;
use mana_viz::logging::util::FrameSize;

fn example_log_detections(rec: &rerun::RecordingStream) {
    // -- Build a fake batch with 2 detections --
    let mut batch = DetectionBatchV1 {
        timestamp_ns: 42_000_000,
        count: 2,
        ..Default::default()
    };
    batch.detections[0] = DetectionV1 {
        cx: 0.3, cy: 0.4, w: 0.15, h: 0.25, confidence: 0.87, class_id: 0,
    };
    batch.detections[1] = DetectionV1 {
        cx: 0.7, cy: 0.6, w: 0.10, h: 0.20, confidence: 0.42, class_id: 1,
    };

    let frame = FrameSize::new(1920, 1080);

    logging::boxes::log_detections_2d(
        rec,
        "/world/camera/detections",   // entity path prefix
        &batch,
        frame,
    )
    .unwrap();
}
```

This produces two entities in Rerun:

- `/world/camera/detections/0/0` -- class=0, label=`"cls:0 cf:0.87"`, box at pixel (576, 432) with half-size (144.0, 135.0)
- `/world/camera/detections/1/0` -- class=1, label=`"cls:1 cf:0.42"`, box at pixel (1344, 648) with half-size (96.0, 108.0)

Both appear overlaid on the camera image in the `"Camera"` `Spatial2DView`.

---

## Summary of all relevant files

| File | Purpose |
|---|---|
| `/home/care/opt/workspace/references/mana-lite/std/mana-viz/src/logging/boxes.rs` | `log_detections_2d`, `log_zones_2d`, `log_roi_2d`, `single_box` |
| `/home/care/opt/workspace/references/mana-lite/std/mana-viz/src/logging/util.rs` | `FrameSize`, `log_archetype`, `log_at`, `log_many` |
| `/home/care/opt/workspace/references/mana-lite/std/mana-viz/src/logging/frame.rs` | `log_frame_rgb24` using `rerun::Image` |
| `/home/care/opt/workspace/references/mana-lite/std/mana-viz/src/logging/mod.rs` | Module declarations |
| `/home/care/opt/workspace/references/mana-lite/std/mana-types/src/lib.rs` | `DetectionV1`, `DetectionBatchV1`, `bbox::box_halfsize_to_pixels` |
| `/home/care/opt/workspace/references/mana-lite/src/viz.rs` | `VizBridge` wrapping everything; `log_detections` calls `log_detections_2d` |
| `/home/care/opt/workspace/references/mana-lite/std/mana-viz/Cargo.toml` | Dependencies: `rerun = "0.35"`, `mana-types` |
