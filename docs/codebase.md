> **Nota:** este es un informe de exploracion historico y puede contener paths
> o estados de sprints anteriores. Para operar o entender el runtime actual,
> usar [operations.md](operations.md), [onboarding.md](onboarding.md) y
> [ARCHITECTURE.md](ARCHITECTURE.md).

Now I have a thorough picture of the entire codebase. Here is the complete detailed report.

---

# Mana-Lite Codebase: Complete Exploration

---

## 1. Full Project Structure

### Top-level layout

```
/home/care/opt/workspace/references/mana-lite/
├── .claude/settings.local.json
├── .git/
├── .gitignore
├── Cargo.lock
├── Cargo.toml                     ← workspace root (package + 4 sub-crates)
├── README.md
├── metricas.md                    ← Spanish ingestion-metrics guide
├── out.txt
├── docs/
│   ├── onboarding.md
│   ├── ROADMAP.md
│   ├── SPEC.md
│   ├── ARCHITECTURE.md
│   ├── adrs/
│   │   ├── 001-single-binary.md
│   │   ├── 002-toml-catalog-pattern.md
│   │   ├── 003-plc-superloop.md
│   │   ├── 004-retina-rtsp.md
│   │   ├── 005-cascaded-inference.md
│   │   ├── 006-json-lines-stdout.md
│   │   ├── 007-iframe-gating.md
│   │   ├── 008-ingest-engine.md
│   │   ├── 009-pipeline-design.md
│   │   ├── 010-preprocess-cache.md
│   │   ├── 011-inference-engine.md
│   │   ├── 012-postprocess-pipeline.md
│   │   ├── 013-sort-tracking.md
│   │   ├── 014-zone-engine.md
│   │   ├── 015-fsm-engine.md
│   │   └── 016-cascade-scheduler.md
│   ├── observability.md            ← guia de metricas, viz, JSONL
│   └── rerun/bbox.md
├── models/                        ← ONNX model files (large, not committed to git)
│   ├── yolo26n.onnx               (detect-fast default)
│   ├── yolo26x.onnx               (detect-large)
│   ├── yolo26s.onnx               (detect-v2)
│   ├── yolo26n-pose.onnx          (pose-standard)
│   ├── yolo26n-face.onnx / .pt
│   ├── yolo26*-depth.onnx (l/m/n/s/x)
│   ├── yolo26*-pose.onnx (l/m/s/x)
│   ├── yolo26*-seg.onnx (l/m/n/s)
│   ├── yolo26n-sem.onnx
│   ├── yolov11*.pt (l/m/n/s face)
│   ├── yolov11n-face.onnx
│   ├── yolov12*.onnx (l/m/n/s face)
│   └── bus.jpg, zidane.jpg
├── src/
│   ├── main.rs                    ← entry point, CLI, superloop orchestration
│   ├── config.rs                  ← all TOML deserialization, env overrides, validation
│   ├── ingest.rs                  ← RTSP client, frame queue, keyframe dedup, reconnect
│   ├── snapshot.rs                ← H.264→RGB decoder (ffmpeg), PNG/H264 saver
│   ├── pipeline.rs                ← PipelineState (frame counter, health eval)
│   ├── metrics.rs                 ← MetricsEngine + Health (blind/stale/recovered)
│   ├── infer.rs                   ← ONNX inference engine (ultralytics-inference)
│   ├── cascade.rs                 ← CascadeScheduler — model dependencies & gating
│   ├── track.rs                   ← SORT-like tracker (IoU matching, predict/update)
│   ├── zones.rs                   ← ZoneEngine — spatial region intersection, hysteresis
│   ├── fsm.rs                     ← FsmEngine — state machine evaluation with dwell timers
│   ├── viz.rs                     ← VizBridge — Rerun gRPC bridge (blueprint + frame/bbox/scalar logging)
│   ├── logger/
│   │   ├── mod.rs                 ← Logger (JSONL, rotating files, buffered flush)
│   │   ├── event.rs               ← Event enum + JsonlLevel
│   │   └── serialize.rs           ← manual JSON serializer (no serde_json)
│   └── error.rs                   ← ManaError + ConfigError enums (thiserror)
├── std/                           ← workspace sub-crates
│   ├── mana-types/
│   │   ├── Cargo.toml
│   │   └── src/lib.rs             ← RawFrameV1, DetectionBatchV1, SceneMsgV1, ZoneV1, PixelFormat, etc.
│   ├── mana-rtsp/
│   │   ├── Cargo.toml
│   │   └── src/{lib.rs, h264.rs}  ← h264::contains_idr() helper
│   ├── mana-video/
│   │   ├── Cargo.toml
│   │   └── src/{lib.rs, decoder.rs, format.rs, raw.rs, buffer_pool.rs} ← SoftwareDecoder
│   └── mana-viz/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           └── logging/
│               ├── mod.rs
│               ├── frame.rs       ← log_frame_rgb24 (send image to Rerun)
│               ├── boxes.rs       ← log_detections_2d, log_zones_2d, log_roi_2d
│               ├── event.rs       ← log_scene_event_text
│               ├── text.rs        ← log_static_text
│               └── util.rs        ← FrameSize, log_archetype, log_at, log_many helpers
├── config/
│   ├── mana.toml                  ← AppConfig (source, ingest, inference, health, output, pipeline, viz)
│   ├── models.toml                ← ModelCatalog (4 active models)
│   ├── cascade.toml               ← CascadeConfig (explicit model ordering/dependencies)
│   ├── fsm.toml                   ← FsmCatalog (4 states, 6 transitions)
│   └── zones.toml                 ← ZoneCatalog (4 zones)
├── snapshots/
│   ├── latest_frame.png
│   └── latest_frame.h264
├── logs/
├── target/debug/mana-lite         ← compiled binary
└── target/...
```

**Language:** Rust edition 2024, `rust-version = "1.89"`, workspace with 5 crates.

---

## 2. cascade.rs — CascadeScheduler & CascadeRule

**File:** `/home/care/opt/workspace/references/mana-lite/src/cascade.rs` (160 lines)

### Key Structs

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct CascadeRule {
    pub model: String,              // model key name, e.g. "detect-fast"
    pub requires: Option<String>,   // parent model key (None = root, always eligible)
    pub requires_class: Option<String>, // class name parent must detect, e.g. "person"
}

#[derive(Debug, Deserialize)]
pub struct CascadeConfig {
    pub rules: Vec<CascadeRule>,    // ordered list of cascade rules
}
```

**Internal entry (runtime):**

```rust
struct CascadeEntry {
    requires: Option<String>,
    requires_class: Option<String>,
    last_run_at: Instant,
}
```

### Key Functions

| Function | Purpose |
|----------|---------|
| `CascadeScheduler::from_rules(rules: &[CascadeRule])` | Builds a `HashMap<String, CascadeEntry>` from config rules. Each model key maps to its requires/requires_class with `last_run_at` set to now. |
| `all_models(&self) -> Vec<String>` | Returns all model keys registered in the cascade. |
| `ordered(&self, requested: &[String]) -> Vec<String>` | Topological-sorts models: root models (no `requires`) come first, then children. Ensures parent models execute before their dependents. |
| `should_run(&mut self, model, parent_dets) -> bool` | Gate function: returns `false` if the model is unknown. If `requires` is `None` (root), always returns `true`. If `requires` is set, checks that the parent model has detections AND that at least one detection has the `requires_class`. Also updates `last_run_at`. |

### How Rules Are Loaded

In `main.rs` bootstrap (lines 114-124):

1. If `config.inference.cascade_file` is set, load `CascadeConfig` from the TOML file via `load_config::<CascadeConfig>(path)`.
2. If no file is configured, use a **hardcoded fallback** of 4 rules:
   - `detect-fast` — root (no requires)
   - `detect-large` — root
   - `detect-v2` — root
   - `pose-standard` — requires `detect-fast` and class `"person"`

The currently-configured `cascade.toml` matches exactly this fallback.

### Cascade Logic in the Superloop

In `run_inference()` (main.rs lines 239-275):

1. Get `fsm_models` from the FSM engine (models for the current FSM state), or fall back to `cascade.all_models()` if no FSM.
2. Filter out models whose task is in `disabled_tasks`.
3. Call `cascade.ordered(&fsm_models)` to topo-sort.
4. For each model in order, call `cascade.should_run(model_key, &model_dets)`.
   - If `false` → increment skip counter, continue.
   - If `true` → run inference via `infer.run()`, accumulate detections into `model_dets` (a `HashMap<String, Vec<Detection>>` fed to subsequent `should_run` checks).

---

## 3. main.rs — How the App Bootstraps

**File:** `/home/care/opt/workspace/references/mana-lite/src/main.rs` (398 lines)

### Entry Point

```rust
#[tokio::main]
async fn main() -> Result<()>
```

1. Parses CLI args — accepts `--config <path>` or positional path, or `--version`/`-V`.
2. Loads `AppConfig` from `mana.toml` via `load_app_config()` (which also applies `MANA_*` env var overrides).
3. Calls `App::bootstrap(&config, &config_path)`.
4. Calls `app.run(&config)` — the infinite superloop.

### Bootstrap Sequence (`App::bootstrap`)

| Step | What happens |
|------|-------------|
| 1 | `load_model_catalog()` → deserializes `models.toml` into `ModelCatalog`. |
| 2 | `load_zone_catalog()` → if `zones_file` is set, deserializes `zones.toml`. |
| 3 | `load_fsm_catalog()` → if `fsm_file` is set, deserializes `fsm.toml`. |
| 4 | Validates the default model exists in the catalog. |
| 5 | Validates the FSM (`validate_fsm()` — checks initial state, transitions reference valid states, models referenced exist, zone guards reference valid zones). Returns error if validation fails. |
| 6 | Creates `Logger` — either rotating files (if `save_dir` is set) or stdout. Emits meta_startup and meta_model_loaded events. |
| 7 | `InferEngine::from_catalog()` — loads all ONNX models from the catalog into `HashMap<String, LoadedModel>`. |
| 8 | Creates `Tracker::new()`, `ZoneEngine::from_catalog()`, `FsmEngine::from_catalog()`. |
| 9 | Loads cascade rules (from `cascade.toml` or hardcoded fallback) → builds `CascadeScheduler`. |
| 10 | Builds `model_tasks` map (`model_key → task_type`) from the model catalog. |
| 11 | Creates `IngestEngine<RetinaReader>` using the real RTSP connection from `RetinaReader::connect()`. |
| 12 | `MetricsEngine::new(interval_s)`, `Health::new(data_stale_ms)`, `FrameDecoder::new()`, `SnapshotSaver::new()`. |
| 13 | If `viz.enabled`, creates `VizBridge::new()` — connects to Rerun via gRPC at `rerun_addr`, sends blueprint. |
| 14 | `PipelineState::new(metrics_text)`. |

### Superloop (`App::run`)

Each iteration:

```
1. metrics.tick_cycle()
2. ingest.poll_freshest_keyframe()  → drains all buffered frames, keeps latest keyframe
   If a new keyframe arrives: process_keyframe()
      ├── decoder.decode_timed()     → H.264 → RGB (ffmpeg)
      ├── state.on_keyframe()        → frame_count++, health.touch(), log frame_ingest event
      ├── viz.log_decode_latency()
      ├── snapshot.save()            → write H.264 raw + PNG to snapshots/
      ├── run_inference()            → cascade-driven model execution + tracking
      ├── evaluate_scene()           → zone evaluation + FSM evaluation
      └── flush_viz_metrics()        → track counts, health ms, frame image to Rerun
3. drain_ingest_counters()           → metrics.tick_ingest() + retina counters
4. evaluate_fsm_wildcard()           → check wildcard (*) transitions (e.g. data_stale → blind)
5. state.evaluate_health()           → check blind/stale/recovered, emit log events
6. viz.log_metrics_report()          → per-window metrics to Rerun
7. log.flush()                       → write buffered JSONL events to stdout/file
8. viz.tick()                        → flush Rerun recording stream
9. continue until the RTSP process is stopped or the panic policy exits
```

---

## 4. Metrics/Logging System

### MetricsEngine (`src/metrics.rs`)

**Structs:**

```rust
struct MetricsEngine {
    current: Metrics,        // accumulating counters
    model_order: Vec<String>,
    window_start: Instant,
    report_interval_s: u64,  // default 5s from config
}

struct Metrics {
    cycles, frames_total, keyframes, pframes_dropped,
    inferences, infer_total_us, decode_total_us,
    blind_cycles, timeouts, ssrc_changes, rtp_errors,
    stream_ends, reconnect_attempts,
    ingest_pframes, ingest_dup_keyframes,
    infer_skips, infer_empty, infer_total_dets,
    model_metrics: HashMap<String, PerModelMetrics>,
}

struct PerModelMetrics {
    inferences, infer_total_us, total_dets,
    skips, empty,
    conf_sum, conf_min, bbox_area_sum,
}
```

`take_report()` returns a `(MetricsReport, Vec<String>)` if the report interval has elapsed, resetting counters to defaults.

### MetricsReport (what `log_report()` prints every `report_interval_s`)

Two log lines at `info` level:

```
ingest: {hz} Hz — {keyframes} keyframes in {window_s}s | decode {avg}ms avg | cycles {N}{flags}
infer:  {hz} Hz — {inferences} calls in {window_s}s | {avg}ms avg | {dets} dets
```

Where `flags` includes only non-zero values from: `pframes, dup, timeouts, reconnect, ssrc, rtp, skips, empty`.

### Health Monitor (`src/metrics.rs`, `Health` struct)

Three states with hysteresis:

- **NORMAL** — last frame within `data_stale_ms / 2` (default 5s)
- **STALE** — no frame for > `data_stale_ms / 2` (emits `HealthTransition::Stale`)
- **BLIND** — no frame for > `data_stale_ms` (emits `HealthTransition::Blind`)
- **RECOVERED** — returns from BLIND to NORMAL (emits `HealthTransition::Recovered`)

### Logger (`src/logger/`)

- **Output:** JSONL (one JSON object per line) to stdout or rotating files (`logs/mana-YYYYMMDDTHH.jsonl` or `logs/mana-YYYYMMDD.jsonl`).
- **Rotate modes:** `Hourly`, `Daily`, `Never`.
- **Buffer:** Events accumulate in a `Vec<Event>`, flushed on `log.flush()`.
- **Level filter:** `JsonlLevel` — `Debug`, `Info`, `Quiet`. Frame/Detection/Zone events are Debug-level; Meta/Health/FSM/Metrics are Info-level.

### Event Types Serialized

| Event Variant | JSON Type | Key Fields |
|--------------|-----------|-----------|
| `Meta` | `"type":"meta"` | `event`, `detail`, arbitrary `attrs` key-value pairs |
| `Health` | `"type":"health"` | `event`, optional `frame_id`, `cyc_us`, `msg` |
| `Frame` | `"type":"frame"` | `frame_id`, `is_keyframe`, `decode_ms` |
| `Detection` | `"type":"detection"` | `frame_id`, `model`, `infer_ms`, `det` array of `{class, confidence, bbox}` |
| `Zone` | `"type":"zone"` | `zone`, `event` ("occupied"/"vacated"), `class`, `frame_id` |
| `Fsm` | `"type":"fsm"` | `from`, `to`, `trigger`, `dwell_ms` |
| `Metrics` | `"type":"metrics"` | `window_s`, `cycles`, `keyframes`, `inferences`, `infer_total_ms`, `decode_total_ms`, `blind_cycles`, `timeouts`, `ssrc_changes`, `rtp_errors`, `stream_ends`, `reconnect_attempts` |

Per-model metrics (within `metrics` event): NOT serialized in JSONL (they go to Rerun only).

Every JSONL line starts with `{"t":"RFC3339_timestamp", ...}`.

### Track Events (via `track_event_to_log`)

- `track_created` — Meta event with class, frame_id, bbox
- `track_updated` — Meta event with frame_id, bbox
- `track_lost` — Meta event with class, frame_id
- `track_deleted` — Meta event with class, reason, frame_id

### Zone Events (via `zone_event_to_log`)

- `zone_occupied` — Zone event with zone name, class, frame_id
- `zone_vacated` — Zone event with zone name, class, frame_id

---

## 5. Rerun Visualization — What Data Is Sent

**File:** `/home/care/opt/workspace/references/mana-lite/src/viz.rs` (252 lines)

### Connection

`VizBridge::new(rerun_addr)` connects via gRPC to `rerun+http://{addr}/proxy`. Configures a 32 MB inflight buffer.

### Blueprint (sent once on startup)

A `rerun::blueprint::Blueprint` with 5 vertically-stacked panels:

```
┌─────────────────────────────────────┐
│  Spatial2DView "Camera"             │  62% height — /world/camera, shows " + $origin/**"
│  [/world/camera]                    │
├─────────────────────────────────────┤
│  TimeSeriesView "Signals"           │  12.5% — /world/signals (frame_id with StepAfter interp)
│  [/world/signals]                   │
├───────────────┬─────────────────────┤
│ TimeSeries    │  TimeSeries         │  12.5% each
│ "Ingest"      │  "Ingest ❌"        │  /ingest/normal, /ingest/errors
│ [/ingest/normal]│ [/ingest/errors]  │
├───────────────┴─────────────────────┤
│  TimeSeries "Infer" / "Infer ❌"    │  12.5% — /infer/active, /infer/warnings
├─────────────────────────────────────┤
│  TimeSeries "Pipeline"              │  12.5% — /pipeline
└─────────────────────────────────────┘
```

### Per-Frame Data (every keyframe)

| Path | Type | Content |
|------|------|---------|
| `/world/camera/bgr` | `rerun::Image` | RGB24 frame (via `log_frame_rgb24()`) |
| `/world/signals/frame_id` | Scalar | Frame number |
| `/world/signals/latency/viewer_loop_s` | Scalar | Loop latency in seconds |
| `/pipeline/loop_latency_us` | Scalar | Loop latency in microseconds |
| `/pipeline/decode/latency_us` | Scalar | H.264 decode time |
| `/pipeline/infer/{model}/latency_us` | Scalar | Per-model inference latency |
| `/pipeline/track/total` | Scalar | Total track count |
| `/pipeline/track/active` | Scalar | Confirmed active track count |
| `/pipeline/health/ms_since_frame` | Scalar | Milliseconds since last frame |

### Detection Boxes (per frame, per model)

Path: `/world/camera/detections/{model}/{class}/{index}`

Each detection is rendered as a `rerun::Boxes2D` with:

- Center + half-size from bbox
- Label: `"{class} {confidence}"`
- Radius: 2.0

The path is cleared first with `rerun::Clear::recursive()` before re-logging.

### Per-Window Metrics (every `report_interval_s`)

**Ingest Normal (`/ingest/normal/`):** hz, keyframes, decode_avg_ms, pframes_dropped

**Ingest Errors (`/ingest/errors/`):** timeouts, ssrc_changes, rtp_errors, reconnect_attempts, dup_keyframes

**Infer Active (`/infer/active/`):** hz, avg_ms, yield_avg

**Infer Warnings (`/infer/warnings/`):** skips, empty

**Per-model (`/infer/{model_safe}/`):**

- `active/` — hz, avg_ms, yield_avg
- `warnings/` — skips, empty
- `detections/` — conf_avg, conf_min, area_avg

**Pipeline (`/pipeline/`):** cycles_window, health/blind_cycles

### Flush

`viz.tick()` calls `rec.flush_with_timeout(100ms)`. If the viewer is offline, it logs a warning every 30 seconds.

---

## 6. Config Files — Complete Contents

### 6a. mana.toml

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
```

Key observations: Tracking, zones, and FSM are **all disabled** in pipeline config. Only inference and viz are active.

### 6b. models.toml

**File:** `/home/care/opt/workspace/references/mana-lite/config/models.toml`

```toml
[models.detect-fast]
path = "models/yolo26n.onnx"
task = "detect"
confidence = 0.5
imgsz = 640

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
```

4 active models. Default `confidence`/`iou`/`max_det` values apply (0.25/0.7/300). 5 other models commented out (face-v11, face-v12, segment-small, depth-small).

### 6c. cascade.toml

**File:** `/home/care/opt/workspace/references/mana-lite/config/cascade.toml`

```toml
[[rules]]
model = "detect-fast"

[[rules]]
model = "detect-large"

[[rules]]
model = "detect-v2"

[[rules]]
model = "pose-standard"
requires = "detect-fast"
requires_class = "person"
```

3 root models (no requires), 1 dependent model (pose-standard depends on detect-fast detecting "person").

### 6d. fsm.toml

**File:** `/home/care/opt/workspace/references/mana-lite/config/fsm.toml`

```toml
[fsm]
initial = "idle"

[fsm.states.idle]
label = "Room Empty"
models = ["detect-fast"]

[fsm.states.watching]
label = "Person Present"
models = ["detect-fast", "pose-standard"]

[fsm.states.bed_alert]
label = "ALERT: Bed Exit Attempt"
models = ["detect-fast", "pose-standard"]
dwell_min_ms = 3_000

[fsm.states.blind]
label = "BLIND: No Camera Signal"

# Wildcard → blind on data stale
[[fsm.transitions]]
from = "*"
to = "blind"
guards = [{ type = "data_stale" }]

# Idle ↔ Watching
[[fsm.transitions]]
from = "idle"
to = "watching"
guards = [
    { type = "zone_occupied", zone = "bed", min_confidence = 0.5 },
    { type = "zone_occupied", zone = "chair", min_confidence = 0.5 }
]

[[fsm.transitions]]
from = "watching"
to = "idle"
guards = [{ type = "all_zones_vacant", min_duration_ms = 60_000 }]

# Watching → Bed Alert
[[fsm.transitions]]
from = "watching"
to = "bed_alert"
guards = [{ type = "zone_vacated", zone = "bed", min_duration_ms = 3_000 }]

# Bed Alert → Watching (recovery)
[[fsm.transitions]]
from = "bed_alert"
to = "watching"
guards = [
    { type = "zone_occupied", zone = "bed", min_duration_ms = 5_000 }
]

# Bed Alert dwell timeout
[[fsm.transitions]]
from = "bed_alert"
to = "blind"
dwell = "5m"
```

### 6e. zones.toml

**File:** `/home/care/opt/workspace/references/mana-lite/config/zones.toml`

```toml
[zones.bed]
x1 = 100; y1 = 200; x2 = 500; y2 = 800
label = "Bed A"
hysteresis_ms = 500

[zones.chair]
x1 = 600; y1 = 300; x2 = 750; y2 = 600
label = "Chair 1"
hysteresis_ms = 1000

[zones.door]
x1 = 0; y1 = 0; x2 = 150; y2 = 900
label = "Entrance Door"
hysteresis_ms = 2000

[zones.floor]
x1 = 0; y1 = 500; x2 = 800; y2 = 900
label = "Floor Area"
hysteresis_ms = 1000
```

---

## 7. Model Files in models/ Directory

**Directory:** `/home/care/opt/workspace/references/mana-lite/models/`

### ONNX files (YOLOv26 nano-to-xlarge variants)

| File | Size class | Task | Used by model key |
|------|-----------|------|-------------------|
| `yolo26n.onnx` | nano | detect | **detect-fast** (active) |
| `yolo26x.onnx` | xlarge | detect | **detect-large** (active) |
| `yolo26s.onnx` | small | detect | **detect-v2** (active) |
| `yolo26n-pose.onnx` | nano | pose | **pose-standard** (active) |
| `yolo26n-face.onnx` | nano | face/detect | (commented out in models.toml) |
| `yolo26l.onnx` | large | detect | unused |
| `yolo26m.onnx` | medium | detect | unused |
| `yolo26m-pose.onnx` | medium | pose | unused |
| `yolo26m-seg.onnx` | medium | segment | unused |
| `yolo26s-pose.onnx` | small | pose | unused |
| `yolo26s-seg.onnx` | small | segment | unused |
| `yolo26s-depth.onnx` | small | depth | unused |
| `yolo26l-depth.onnx` | large | depth | unused |
| `yolo26m-depth.onnx` | medium | depth | unused |
| `yolo26n-depth.onnx` | nano | depth | unused |
| `yolo26n-seg.onnx` | nano | segment | unused |
| `yolo26n-sem.onnx` | nano | semantic | unused |
| `yolo26x-depth.onnx` | xlarge | depth | unused |
| `yolo26x-pose.onnx` | xlarge | pose | unused |

### YOLO Face v11/v12 variants

| File | Variant | Format |
|------|---------|--------|
| `yolov11n-face.onnx` | v11 nano | ONNX |
| `yolov11n-face.pt` | v11 nano | PyTorch |
| `yolov11s-face.pt` | v11 small | PyTorch |
| `yolov11m-face.pt` | v11 medium | PyTorch |
| `yolov11l-face.pt` | v11 large | PyTorch |
| `yolov12n-face.onnx` | v12 nano | ONNX |
| `yolov12s-face.onnx` | v12 small | ONNX |
| `yolov12m-face.onnx` | v12 medium | ONNX |
| `yolov12l-face.onnx` | v12 large | ONNX |

### Other files

- `bus.jpg`, `zidane.jpg` — test images
- `yolo26n-face.pt` — PyTorch source for the nano face model
- `yolo-face-1.0.0/` — a subdirectory (appears to be the yolo-face upstream repo)
- `.git/`, `.github/`, `docs/`, `examples/`, `src/`, `target/`, `tests/`, `runs/`, `assets/` — embedded in the models directory (suggesting `models/` was cloned as a sub-repo or contains reference implementation files)

---

## Summary of Key Architectural Patterns

1. **Single-threaded PLC superloop** — no channels, no spawn, no IPC. One `App` struct with all state, one `loop { ... }`.

2. **Cascade scheduling** — models declare dependencies (`requires` + `requires_class`). The scheduler topologically sorts root-first, then conditionally executes dependent models only when parent detections match.

3. **FSM-driven model selection** — each FSM state declares which models it needs. The cascade then filters those further.

4. **Keyframe-only ingestion** — P-frames are always dropped. Only the freshest I-frame per cycle is used. Duplicate consecutive keyframes are suppressed.

5. **Health monitoring** — three-state (normal/stale/blind) with hysteresis prevents oscillation. Blind state triggers FSM wildcard transitions.

6. **Manual JSONL serialization** — avoids `serde_json` dependency, uses a reusable `Vec<u8>` buffer, writes u64 without string formatting.

7. **Dual-channel metadata output** — JSONL to stdout/files for downstream consumers (the "clinical record"), Rerun gRPC for real-time operator visualization.

8. **All pipeline stages are toggleable** — `infer`, `track`, `zones`, `fsm`, `snapshot` can each be independently enabled/disabled in `mana.toml`.
