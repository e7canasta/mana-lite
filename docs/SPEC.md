# Mana Lite Specification

## CLI

```
mana-lite --config mana.toml
mana-lite mana.toml
mana-lite --version
```

## Configuration Schemas

### `mana.toml` — Application Root

```toml
[source]
url = "rtsp://192.168.1.100:554/stream1"
username = "admin"
password = ""
transport = "tcp"           # "tcp" | "udp"
keyframes_only = true

[inference]
model_catalog = "config/models.toml"
blueprint_file = "config/blueprints/detect-face/blueprint.toml"
zones_file = "config/zones.toml"
fsm_file = "config/fsm.toml"

[tracking]
min_hits = 2
max_age = 20
tentative_max_age = 3
iou_threshold = 0.2

[health]
data_stale_ms = 10_000      # time without frame → BLIND
max_consecutive_panics = 3
report_interval_s = 5

[output]
format = "jsonl"            # "jsonl" only for v0.x
save_dir = "./logs"         # optional, omit for stdout-only
rotate = "hourly"           # "hourly" | "daily" | "never"
snapshot_dir = "./snapshots"
snapshot_verbose = false
jsonl_level = "info"
```

### `models.toml` — Model Catalog

The active inference profile is selected separately with
`inference.blueprint_file`. See [specs/inference-blueprints.md](specs/inference-blueprints.md).

Each table under `[models]` is a named entry. Keys are stable identifiers for FSM/stage to reference.

```toml
[models.detect-fast]
path = "models/yolo26n.onnx"
task = "detect"
enabled = true          # default true; false deshabilita la rama en el cascade
confidence = 0.5
iou = 0.5
max_det = 100
imgsz = 320
device = "cpu"
half = false            # FP16 inference

[models.detect-fast.postprocess]
allow_classes = ["person", "wheelchair"]
min_confidence = 0.25
min_area_ratio = 0.001
max_area_ratio = 1.0

[models.pose-standard]
path = "tools/model-tools/artifacts/yolo26-fp16/yolo26s-pose-fp16-320.onnx"
task = "pose"
confidence = 0.3
iou = 0.5
imgsz = 320
half = true

[models.face-yolo]
path = "models/yolov12l-face.onnx"
task = "detect"
confidence = 0.1

[models.seg-standard]
path = "models/yolo26x-seg-fp16-640.onnx"
task = "detect"
confidence = 0.2
half = true

[models.depth-standard]
path = "tools/model-tools/artifacts/yolo26-fp16/yolo26x-depth-fp16-320.onnx"
task = "depth"
confidence = 0.0
imgsz = 320
half = true

[models.depth-standard.crop]
type = "static"
region = [560, 140, 1240, 820]
```

**Fields:**

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `path` | string | yes | — | Filesystem path to `.onnx` |
| `task` | string | yes | — | `detect`, `segment`, `pose`, `classify`, `obb`, `semantic`, `depth` |
| `enabled` | bool | no | true | `false` deshabilita el modelo en el scheduler (ADR-020) |
| `confidence` | float | no | 0.25 | Detection confidence threshold |
| `iou` | float | no | 0.7 | NMS IoU threshold |
| `max_det` | int | no | 300 | Max detections per frame |
| `imgsz` | int | no | from ONNX metadata | Input size (square) |
| `device` | string | no | `"cpu"` | `cpu`, `rocm:0`, `openvino` |
| `half` | bool | no | false | FP16 inference |
| `rect` | bool | no | true | Rectangular (aspect-preserving) preprocessing |
| `polygon_simplify` | float | no | 0.75 | Simplificación RDP de polígonos de máscara |
| `postprocess.allow_classes` | string[] | no | `[]` | Classes published by this model; empty allows all |
| `postprocess.min_confidence` | float | no | 0.0 | Additional output confidence threshold |
| `postprocess.min_area_ratio` | float | no | 0.0 | Minimum bbox area relative to original frame |
| `postprocess.max_area_ratio` | float | no | 1.0 | Maximum bbox area relative to original frame |
| `postprocess.min_component_area_ratio` | float | no | 0.0 | Minimum connected mask-component area relative to the detection mask crop |
| `postprocess.mask_threshold` | float | no | 0.5 | Foreground threshold for segmentation masks |
| `postprocess.nms_iou` | float | no | 0.5 | Explicit mana-lite NMS IoU after class/conf/area filters |
| `postprocess.max_detections` | int | no | — | Keep top-K by confidence after NMS |
| `crop` | table | no | — | Static or `largest_class` crop (ver [roi.md](roi.md)) |

The model-level `confidence` and `iou` values configure the inference engine
and its intra-model NMS. Each model can define its own postprocessing filters;
they run afterward on that model's unified detections.

For segmentation models, `min_component_area_ratio` removes disconnected
foreground components smaller than the configured fraction of the detection
mask crop. The cleaned raster is used to build both `CompactMask` and
polygons, so the lossless mask and its derived contours remain aligned. Large
disconnected components remain part of the same detection.

Models may also define a physical crop. A static crop uses original-frame
coordinates; a dynamic `largest_class` crop is resolved from the accepted
cascade target.

```toml
[models.pose-standard.crop]
type = "largest_class"
class = "person"
margin = 0.15

[models.pose-standard.postprocess]
allow_classes = ["person"]
min_confidence = 0.3
min_area_ratio = 0.001
max_area_ratio = 1.0
```

`allow_classes = []` allows every class for that model. This supports models
with different roles: a person detector can emit `person`, a wheelchair
detector can emit `wheelchair`, a face model `face`, and a depth model can
leave the allowlist empty.

### `[presence]` en `mana.toml` — Temporal Presence Signal

```toml
[presence]
enabled = true
class = "person"
on_ticks = 1
off_ticks = 4
```

This filter runs after spatial consolidation of primary observations and before
the classic tracker. Empty valid inference ticks shorter than `off_ticks` hold
the last person observation. Missing/invalid input does not count as absence.
Two or more accepted persons enter an ambiguous state immediately and are not
held as a single person.

### `cascade.toml` - Cascaded Model Eligibility

The cascade uses confirmed tracks from the parent model. A single-frame
detection cannot activate a child model.

```toml
[regions.bed]
rect = [100, 200, 500, 800]
label = "Bed A"

[[rules]]
model = "detect-fast"

[[rules]]
model = "depth-standard"          # root independiente: sin requires

[[rules]]
model = "pose-standard"
requires = "detect-fast"
requires_class = "person"
requires_min_confidence = 0.50
requires_min_area_ratio = 0.01
requires_region = "bed"
requires_region_coverage = 0.30
same_frame = true

[[rules]]
model = "face-yolo"
requires = "detect-fast"
requires_class = "person"
requires_exact_count = 1
same_frame = true
```

`same_frame = true` usa las detecciones del parent del mismo frame, sin
necesitar un track confirmado. `requires_region_coverage` es la intersección
entre el bbox del track y la región semántica dividida por el área del bbox;
no es IoU, así una persona pequeña dentro de una región grande puede cumplir
la regla.

El modelo crop permanece independiente de la elegibilidad semántica. Por
ejemplo, `pose-standard.crop` puede recortar al bbox de la persona aceptada
después de que el cascade aprobó ese track.

`depth-standard` es un root deliberado: corre aunque no haya detecciones, no
entra en consolidación, tracking, zonas ni FSM. Su validación es
`valid_pixels`, no detecciones (ver [specs/depth-standard.md](specs/depth-standard.md)).

### Detection Consolidation

Model detections are not scene entities. The pipeline uses three levels:

```text
Detection          one model output in one cycle
ConsolidatedObservation  spatial consolidation for the current cycle
TrackedEntity            temporal identity with multi-rate evidence
```

Detections from the same class can fuse by IoU. `pose`, `face` and `segment`
can enrich a primary entity without creating a second scene bbox. Face uses
containment over the face bbox rather than ordinary IoU because the face is a
component inside the person bbox.

The current stateless output is published once per `ConsolidatedObservation`:
JSONL uses `consolidated_detection` and Rerun uses
`/world/camera/observations`. Model-specific detections remain available as
diagnostic events.

When `pipeline.track = true`, the tracker additionally publishes the canonical
bbox as a `TrackedEntity` and Rerun can show it in the entity layer. Therefore
an observation and a tracked entity are deliberately separate outputs, even
when they describe the same subject in one frame.

Independent freshness and TTL for secondary evidence are planned for the
tracking stage. They are not part of the current stateless consolidation mode.

### Depth (ROI-local)

`depth-standard` es un root de cascada con crop estático. El contrato espacial:

- `DepthMap.data` tiene la geometría **local** del ROI (ej. 680x680), nunca
  un buffer full-frame (ADR-024).
- `crop_rect`/`roi` es el origen global. Para consultar una región global
  `[gx1,gy1,gx2,gy2]`: intersectar con el ROI y restar el origen — la fórmula
  está en [specs/depth-standard.md](specs/depth-standard.md) §7.
- Un valor es válido si es finito y mayor que cero. Las reglas no deben
  basarse en un solo pixel.
- El evento JSONL `type=depth` emite estadísticas (valid_pixels, min/max), no
  la matriz completa. `map_space = "roi"` está planificado (spec §10).
- Depth no crea personas, no asigna identidad, no entra en consolidación y no
  debe disparar otros modelos.

### `zones.toml` — Spatial Zones

```toml
[zones.bed]
x1 = 100
y1 = 200
x2 = 500
y2 = 800
label = "Bed A"
hysteresis_ms = 500

[zones.chair]
x1 = 600
y1 = 300
x2 = 750
y2 = 600
label = "Chair 1"
hysteresis_ms = 1000

[zones.door]
x1 = 0
y1 = 0
x2 = 150
y2 = 900
label = "Entrance Door"
hysteresis_ms = 2000
```

Coordinates are in **original image pixels** (before any ROI crop). The zone engine accounts for the ROI offset.

### `fsm.toml` — Clinical State Machine

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
dwell_min_ms = 3000   # minimum time in this state before auto-escalation

[fsm.states.blind]
label = "BLIND: No Camera Signal"

# ── Transitions ──

[[fsm.transitions]]
from = "idle"
to = "watching"
guards = [
    { type = "zone_occupied", zone = "bed", min_confidence = 0.5 },
    { type = "zone_occupied", zone = "chair", min_confidence = 0.5 }
]

[[fsm.transitions]]
from = "watching"
to = "bed_alert"
guards = [
    { type = "zone_vacated", zone = "bed", min_duration_ms = 3000 }
]

[[fsm.transitions]]
from = "watching"
to = "idle"
guards = [
    { type = "all_zones_vacant", min_duration_ms = 60000 }
]

[[fsm.transitions]]
from = "bed_alert"
to = "watching"
guards = [
    { type = "zone_occupied", zone = "bed", min_duration_ms = 5000 },
    { type = "all_zones_vacant", zone = "bed", min_duration_ms = 60000 }
]

# Guard-less transitions (auto-escalation by dwell)
[[fsm.transitions]]
from = "bed_alert"
to = "blind"
dwell = "5m"    # after 5 minutes in alarmed, escalate

[[fsm.transitions]]
from = "*"
to = "blind"
guards = [
    { type = "data_stale" }
]
```

**Guard types:**

| Type | Parameters | Semantics |
|---|---|---|
| `zone_occupied` | `zone`, `min_confidence` | Any detection matching `zone` bbox with conf ≥ threshold |
| `zone_vacated` | `zone`, `min_duration_ms` | Zone was occupied and has been empty for duration |
| `all_zones_vacant` | `min_duration_ms` | All defined zones empty for duration |
| `data_stale` | — | No frame received within `health.data_stale_ms` |

**Dwell semantics:** A guard must evaluate true consistently for `min_duration_ms` before the transition fires. Counter resets on any false evaluation.

---

## Superloop Phases

| Phase | Action | WCET target |
|---|---|---|
| `TIMERS` | Advance `Ton`/`Tof` dwell counters, check stale timers | < 1µs |
| `EVALUATE` | Evaluate FSM guards against current detections + zone state | < 10µs |
| `INGEST` | Read one frame from RTSP (non-blocking try-read), decode if keyframe | < 5ms decode |
| `INFER` | Run root models, apply per-model postprocess filters, update tracking, then run eligible child models on confirmed-track crops. | 20-200ms per model |
| `ZONES` | Map detections to spatial zones, update zone occupancy state | < 10µs |
| `FSM` | Evaluate transitions with satisfied dwells, advance state | < 10µs |
| `PUBLISH` | Flush accumulated events to stdout | < 100µs |

The loop runs **as fast as the slowest phase allows** (bottlenecked by INFER). Child models are skipped when no confirmed track satisfies their cascade rule. Timer-based model intervals are not part of the current implementation.

## Event Output (JSONL)

Every line is a complete JSON object terminated by `\n`. All timestamps are ISO 8601 with millisecond precision.

```json
{"t":"2026-08-03T20:15:00.123Z","type":"meta","event":"startup","v":"0.1.0"}
{"t":"2026-08-03T20:15:00.200Z","type":"meta","event":"model_loaded","model":"detect-fast","path":"models/yolo26n.onnx","task":"detect","warmup_ms":340}
{"t":"2026-08-03T20:15:00.334Z","type":"health","event":"heartbeat","f":0,"ph":"timers","cyc_us":12}
{"t":"2026-08-03T20:15:00.450Z","type":"frame","f":1,"kf":true,"dec_ms":18}
{"t":"2026-08-03T20:15:00.520Z","type":"detection","f":1,"m":"detect-fast","inf_ms":52,"pipeline_ms":58,"post_rejected":2,"post_nms_suppressed":1,"det":[{"c":"person","conf":0.87,"bb":[100,200,300,500]}]}
{"t":"2026-08-03T20:15:00.525Z","type":"consolidated_detection","frame_id":1,"class":"person","confidence":0.87,"bbox":[100,200,300,500],"primary_model":"detect-fast","sources":["detect-fast"]}
{"t":"2026-08-03T20:15:00.528Z","type":"depth","frame_id":1,"model":"depth-standard","infer_ms":268,"pipeline_ms":281,"width":680,"height":680,"valid_pixels":462400,"min_depth_m":2.19,"max_depth_m":5.16}
{"t":"2026-08-03T20:15:00.530Z","type":"entity","track_id":7,"class":"person","bbox":[100,200,300,500],"sources":["detect-fast"]}
{"t":"2026-08-03T20:15:00.535Z","type":"zone","z":"bed","e":"occupied","cls":"person","f":1}
{"t":"2026-08-03T20:15:00.540Z","type":"fsm","from":"idle","to":"watching","tr":"bed_occupied","dwell":0}
{"t":"2026-08-03T20:15:02.500Z","type":"fsm","from":"watching","to":"bed_alert","tr":"bed_exit_attempt","dwell":3000}
{"t":"2026-08-03T20:15:05.000Z","type":"health","event":"stale","c":"ingest","ms":4500}
{"t":"2026-08-03T20:15:10.000Z","type":"health","event":"blind","msg":"No frame for 10000ms"}
{"t":"2026-08-03T20:30:00.000Z","type":"meta","event":"shutdown","reason":"SIGTERM","uptime":900}
```

**Field key abbreviations** (reduce line size for high-throughput):
- `t` = timestamp, `f` = frame, `kf` = keyframe, `dec_ms` = decode_ms
- `m` = model, `inf_ms` = backend inference time, `pipeline_ms` = wall-clock model time, `post_rejected` = detections removed by model filters, `post_nms_suppressed` = detections removed by explicit mana-lite NMS, `det` = detections
- `track_id` = tracked temporal entity identifier, `sources` = contributing models
- `consolidated_detection` has no identity; it represents only the current frame
- `c` = class, `conf` = confidence, `bb` = bbox
- `z` = zone, `e` = event, `cls` = by_class
- `tr` = trigger, `ph` = phase, `cyc_us` = cycle_us
- `v` = version, `c` = component (in health)

## Error Handling

- **Model load failure:** emit `meta/model_load_failed`, skip that model, continue with remaining models. FSM guards referencing failed models are treated as `false`.
- **RTSP disconnect:** emit `health/stale`, enter reconnect loop with exponential backoff (1s, 2s, 4s, 8s, max 30s). During reconnect, FSM evaluates without new frames → will transition to `blind` after `data_stale_ms`.
- **ONNX inference panic:** caught at phase boundary. Increment error counter. If counter exceeds `max_consecutive_panics`, emit `health/blind` and skip that model for N cycles.
- **Guard eval panic:** caught at phase boundary. Treat as `false` for that cycle. Increment error counter.
