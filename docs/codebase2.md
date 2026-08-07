Now I have a thorough picture. Here is the complete summary of every file, struct, and default value.

---

## 1. `config/models.toml` (full)

**File:** `/home/care/opt/workspace/references/mana-lite/config/models.toml`

```toml
[models.detect-fast]
path = "models/yolo26n.onnx"
task = "detect"
confidence = 0.5
imgsz = 640
# Optional: static ROI — only look at part of the frame
# [models.detect-fast.crop]
# type = "static"
# region = [100, 50, 500, 400]

[models.detect-large]
path = "models/yolo26x.onnx"
task = "detect"
confidence = 0.3
imgsz = 640

[models.detect-v2]
path = "models/yolo26s.onnx"
task = "detect"
confidence = 0.3
imgsz = 640

[models.pose-standard]
path = "models/yolo26n-pose.onnx"
task = "pose"
confidence = 0.3
imgsz = 640
# Optional: dynamic ROI — crop to largest "person" bbox from parent model
# [models.pose-standard.crop]
# type = "largest_class"
# class = "person"
# margin = 0.15
#
# Full ROI example with all options:
# [models.bed-detector.crop]
# type = "largest_class"
# class = "person"
# margin = 0.15                       # expand bbox by 15% in each direction
# min_region = [100, 200, 500, 450]   # never smaller than this (bed area)
# max_region = [0, 0, 640, 400]       # never exceed this (exclude door at bottom)
# fallback = "full"                   # if no person, run on full frame (default: "skip")
```

---

## 2. `config/mana.toml` (full)

**File:** `/home/care/opt/workspace/references/mana-lite/config/mana.toml`

```toml
[source]
url = "rtsp://192.168.1.6:8554/clip1"
username = "admin"
password = ""
transport = "tcp"
keyframes_only = true

[ingest]
poll_timeout_ms = 50
error_window_size = 128
error_window_threshold = 25
reconnect_backoff_initial_ms = 1000
reconnect_backoff_max_ms = 30000

[inference]
model_catalog = "config/models.toml"
default_model = "detect-fast"
cascade_file = "config/cascade.toml"
zones_file = "config/zones.toml"
fsm_file = "config/fsm.toml"

[health]
data_stale_ms = 10_000
max_consecutive_panics = 3
report_interval_s = 5

[output]
format = "jsonl"
save_dir = "./logs"
rotate = "hourly"
snapshot_dir = "./snapshots"
snapshot_verbose = false
jsonl_level = "debug"

[pipeline]
snapshot = false
infer = true
track = false
zones = false
fsm = false

[viz]
enabled = true
rerun_addr = "127.0.0.1:9876"

metrics_file = "config/metrics.toml"
viz_file = "config/viz.toml"
rerun_file = "config/rerun.toml"
```

---

## 3. `config/cascade.toml` (full)

**File:** `/home/care/opt/workspace/references/mana-lite/config/cascade.toml`

```toml
[[rules]]
model = "detect-fast"

[[rules]]
model = "detect-large"

[[rules]]
model = "detect-v2"
```

All three models are roots (no `requires`/`requires_class`). All three run every frame.

---

## 4. `config/metrics.toml` (full)

**File:** `/home/care/opt/workspace/references/mana-lite/config/metrics.toml`

```toml
[metrics]
report_interval_s = 5

[metrics.text]
ingest_line = true          # ingest: Hz, keyframes, decode, cycles, flags
infer_summary = true        # infer:  Hz, calls, latency, dets, flags
per_model_lines = true      # per-model detail lines (hz, latency range, dets/fr)

[metrics.text.flags]
ingest_pframes = true
ingest_dup = true
ingest_timeouts = true
ingest_reconnect = true
ingest_ssrc = true
ingest_rtp = true
infer_skips = true
infer_empty = true

[metrics.jsonl]
frame_events = true         # {"type":"frame", frame_id, decode_ms}
detection_events = true     # {"type":"detection", model, infer_ms, det:[...]}
zone_events = true          # {"type":"zone", zone, event, class, confidence}
fsm_events = true           # {"type":"fsm", from, to, trigger, dwell_ms}
metrics_event = true        # {"type":"metrics", ...} — periodic window report
per_model_in_window = true  # include per-model stats inside metrics event
class_counts_in_window = true # include per-class counts inside per-model stats
class_per_frame_stats = true  # include per-class counts/conf/area in detection events
```

---

## 5. `config/viz.toml` (full)

**File:** `/home/care/opt/workspace/references/mana-lite/config/viz.toml`

```toml
[viz]
enabled = true
rerun_addr = "127.0.0.1:9876"

[viz.send]
frames = true                       # RGB image per keyframe
boxes = true                        # detection bounding boxes
decode_latency = true               # H.264 decode time per frame
infer_latency = true                # per-model inference time per frame
class_counts_per_frame = true       # per-class detection count per frame
class_confidence_per_frame = true   # per-class conf min/max per frame
class_area_per_frame = true         # per-class bbox area min/max per frame
keyframe_gap = true                 # ms between keyframes
```

---

## 6. `config/rerun.toml` (full)

**File:** `/home/care/opt/workspace/references/mana-lite/config/rerun.toml`

```toml
[rerun]
app = "mana-lite"
max_bytes_in_flight_mb = 32
auto_views = false
panels_expanded = true

[[rerun.rows]]
kind = "spatial2d"
name = "Camera"
origin = "/world/camera"
share = 5.0

[[rerun.rows]]
kind = "horizontal"
name = "Per-Class Activity"
share = 1.0
panels = [
    { kind = "timeseries", name = "Counts",     origin = "/infer", contents = ["+ /infer/**/per_frame/counts/**"] },
    { kind = "timeseries", name = "Confidence",  origin = "/infer", contents = ["+ /infer/**/per_frame/conf/**"] },
    { kind = "timeseries", name = "Area",        origin = "/infer", contents = ["+ /infer/**/per_frame/area/**"] },
]

[[rerun.rows]]
kind = "horizontal"
name = "Pipeline Health"
share = 1.0
panels = [
    { kind = "timeseries", name = "Latency", origin = "/pipeline",     contents = ["+ /pipeline/infer/**/latency_us", "+ /pipeline/decode/latency_us"] },
    { kind = "timeseries", name = "Stream",  origin = "/ingest/normal", contents = ["+ /ingest/normal/gap_ms"] },
]
```

---

## 7. ONNX Model Files in `models/`

The `models/` directory is a **git submodule** pointing to another repo. The listing shows these ONNX files (they would only exist if the submodule has been initialized/checked out):

| Category | Files |
|---|---|
| **Detection (nano/small/medium/large/xlarge)** | `yolo26n.onnx`, `yolo26s.onnx`, `yolo26m.onnx`, `yolo26l.onnx`, `yolo26x.onnx` |
| **Pose** | `yolo26n-pose.onnx`, `yolo26s-pose.onnx`, `yolo26m-pose.onnx`, `yolo26x-pose.onnx` |
| **Segmentation** | `yolo26n-seg.onnx`, `yolo26s-seg.onnx`, `yolo26m-seg.onnx` |
| **Depth** | `yolo26n-depth.onnx`, `yolo26s-depth.onnx`, `yolo26m-depth.onnx`, `yolo26l-depth.onnx`, `yolo26x-depth.onnx` |
| **Face (YOLOv11/v12)** | `yolov11n-face.onnx`, `yolov12l-face.onnx`, `yolov12m-face.onnx`, `yolov12n-face.onnx`, `yolov12s-face.onnx` |
| **Semantic** | `yolo26n-sem.onnx` |
| **Face (YOLO26)** | `yolo26n-face.onnx` |

The currently active models in `models.toml` reference: `yolo26n.onnx`, `yolo26x.onnx`, `yolo26s.onnx`, `yolo26n-pose.onnx`.

---

## 8. Struct Definitions for Metrics/Viz Configs (with Defaults)

All defined in `/home/care/opt/workspace/references/mana-lite/src/config.rs`:

### `VizConfig` (from `mana.toml`)

```rust
pub struct VizConfig {
    pub enabled: bool,        // default: false
    pub rerun_addr: String,   // default: "0.0.0.0:9876"
}
```

### `VizDataConfig` (from `viz.toml`)

```rust
pub struct VizDataConfig {
    pub viz: VizDataInner,
}

pub struct VizDataInner {
    pub enabled: Option<bool>,       // default: None
    pub rerun_addr: Option<String>,  // default: None
    pub send: VizSendToggles,        // default: all true
}

pub struct VizSendToggles {
    pub frames: bool,                    // default: true
    pub boxes: bool,                     // default: true
    pub decode_latency: bool,            // default: true
    pub infer_latency: bool,             // default: true
    pub class_counts_per_frame: bool,    // default: true
    pub class_confidence_per_frame: bool,// default: true
    pub class_area_per_frame: bool,      // default: true
    pub keyframe_gap: bool,              // default: true
}
```

### `MetricsLogConfig` (from `metrics.toml`)

```rust
pub struct MetricsLogConfig {
    pub metrics: MetricsInner,
}

pub struct MetricsInner {
    pub report_interval_s: u64,     // default: 5
    pub text: MetricsTextConfig,    // default: all true
    pub jsonl: MetricsJsonlConfig,  // default: all true
}

pub struct MetricsTextConfig {
    pub ingest_line: bool,     // default: true
    pub infer_summary: bool,   // default: true
    pub per_model_lines: bool, // default: true
    pub flags: MetricsTextFlags,
}

pub struct MetricsTextFlags {
    pub ingest_pframes: bool,     // default: true
    pub ingest_dup: bool,         // default: true
    pub ingest_timeouts: bool,    // default: true
    pub ingest_reconnect: bool,   // default: true
    pub ingest_ssrc: bool,        // default: true
    pub ingest_rtp: bool,         // default: true
    pub infer_skips: bool,        // default: true
    pub infer_empty: bool,        // default: true
}

pub struct MetricsJsonlConfig {
    pub frame_events: bool,              // default: true
    pub detection_events: bool,          // default: true
    pub zone_events: bool,               // default: true
    pub fsm_events: bool,                // default: true
    pub metrics_event: bool,             // default: true
    pub per_model_in_window: bool,       // default: true
    pub class_counts_in_window: bool,    // default: true
    pub class_per_frame_stats: bool,     // default: true
}
```

### `RerunBlueprintConfig` (from `rerun.toml`)

```rust
pub struct RerunBlueprintConfig {
    pub rerun: RerunRoot,
}

pub struct RerunRoot {
    pub app: String,                     // default: "mana-lite"
    pub max_bytes_in_flight_mb: usize,   // default: 32
    pub auto_views: bool,                // default: true
    pub panels_expanded: bool,           // default: true
    pub rows: Vec<RerunRow>,             // default: empty
}

pub struct RerunRow {
    pub kind: String,          // e.g. "spatial2d", "horizontal"
    pub name: String,          // default: ""
    pub origin: String,        // default: ""
    pub share: f32,            // default: 0.0
    pub overrides: Vec<RerunOverride>,
    pub panels: Vec<RerunPanel>,
}
```

---

## 9. `CropConfig` Struct Definition

From `/home/care/opt/workspace/references/mana-lite/src/config.rs` (lines 220-257):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct CropConfig {
    #[serde(rename = "type")]
    pub crop_type: CropType,              // required - "static" or "largest_class"

    #[serde(default)]
    pub class: Option<String>,            // target class name (for largest_class)

    #[serde(default = "default_crop_margin")]
    pub margin: f32,                      // default: 0.15 (15% expansion)

    #[serde(default)]
    pub region: Option<[u32; 4]>,         // [x1, y1, x2, y2] — for static crop

    #[serde(default)]
    pub min_region: Option<[u32; 4]>,     // lower bound for largest_class union

    #[serde(default)]
    pub max_region: Option<[u32; 4]>,     // upper bound clamp

    #[serde(default)]
    pub fallback: FallbackMode,           // Skip (default) or Full
}

impl CropConfig {
    pub fn always_run(&self) -> bool {
        self.min_region.is_some() || self.fallback == FallbackMode::Full
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CropType {
    Static,        // fixed region — eager ROI set at model load time
    LargestClass,  // per-frame dynamic ROI from parent model's detections
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FallbackMode {
    #[default]
    Skip,  // if target class not found, skip this model
    Full,  // if target class not found, run on the full frame
}

fn default_crop_margin() -> f32 { 0.15 }
```

The related `ModelEntry` defaults (lines 261-265):

```rust
fn default_confidence() -> f32 { 0.25 }
fn default_iou() -> f32 { 0.7 }
fn default_max_det() -> u32 { 300 }
fn default_device() -> String { "cpu".into() }
fn default_rect() -> bool { true }
```

---

## Summary: Current "Minimal Static ROI Test with Detection Only" Configuration

Based on the current config files, the pipeline is configured as:

- **Source:** RTSP stream `rtsp://192.168.1.6:8554/clip1`
- **Default model:** `detect-fast` (yolo26n.onnx, confidence 0.5, imgsz 640, device=cpu)
- **Cascade:** All three detect models (`detect-fast`, `detect-large`, `detect-v2`) run as roots every frame (no cascading dependencies)
- **Pipeline:** `infer = true`, everything else (`track`, `zones`, `fsm`, `snapshot`) is `false` — exactly "detection only"
- **Crop:** All crop configs are **commented out** — no ROI cropping is active. Every model runs on the full frame
- **Output:** JSONL to `./logs/` with `debug` level, rotating hourly
- **Viz/Rerun:** Enabled at `127.0.0.1:9876`, all visualization toggles on
- **Metrics:** 5-second reporting window, all text and JSONL outputs enabled
