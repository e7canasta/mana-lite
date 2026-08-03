# Mana Lite Specification

## CLI

```
mana-lite --config mana.toml
mana-lite --config mana.toml --model detect-fast
mana-lite --config mana.toml --output-dir ./logs
mana-lite replay --mp4 recording.mp4 --config mana.toml
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
default_model = "detect-fast"
zones_file = "config/zones.toml"
fsm_file = "config/fsm.toml"

[health]
data_stale_ms = 10_000      # time without frame → BLIND
max_consecutive_panics = 3
heartbeat_every_n_cycles = 100

[output]
format = "jsonl"            # "jsonl" only for v0.x
save_dir = "./logs"         # optional, omit for stdout-only
rotate = "hourly"           # "hourly" | "daily" | "never"
```

### `models.toml` — Model Catalog

Each table under `[models]` is a named entry. Keys are stable identifiers for FSM/stage to reference.

```toml
[models.detect-fast]
path = "models/yolo26n.onnx"
task = "detect"
confidence = 0.5
iou = 0.7
max_det = 100
imgsz = 320
device = "cpu"

[models.detect-large]
path = "models/yolo26x.onnx"
task = "detect"
confidence = 0.3
imgsz = 640

[models.detect-v2]
path = "models/yolo27n.onnx"
task = "detect"
confidence = 0.3
imgsz = 320

[models.pose-standard]
path = "models/yolo26n-pose.onnx"
task = "pose"
confidence = 0.3
imgsz = 640

[models.face-v11]
path = "models/yolov11n-face.onnx"
task = "detect"
confidence = 0.6
imgsz = 320

[models.face-v12]
path = "models/yolov12n-face.onnx"
task = "detect"
confidence = 0.5
imgsz = 320

[models.segment-large]
path = "models/yolo26x-seg.onnx"
task = "segment"
confidence = 0.3
imgsz = 640

[models.depth-small]
path = "models/depth-anything-small.onnx"
task = "depth"
imgsz = 384
```

**Fields:**

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `path` | string | yes | — | Filesystem path to `.onnx` |
| `task` | string | yes | — | `detect`, `segment`, `pose`, `classify`, `obb`, `semantic`, `depth` |
| `confidence` | float | no | 0.25 | Detection confidence threshold |
| `iou` | float | no | 0.7 | NMS IoU threshold |
| `max_det` | int | no | 300 | Max detections per frame |
| `imgsz` | int | no | from ONNX metadata | Input size (square) |
| `device` | string | no | `"cpu"` | `cpu`, `rocm:0`, `openvino` |
| `half` | bool | no | false | FP16 inference |
| `rect` | bool | no | true | Rectangular (aspect-preserving) preprocessing |

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

[fsm.states.alarmed]
label = "ALERT: Bed Exit Attempt"
models = ["detect-fast", "face-v12"]
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
to = "alarmed"
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
from = "alarmed"
to = "watching"
guards = [
    { type = "zone_occupied", zone = "bed", min_duration_ms = 5000 },
    { type = "all_zones_vacant", zone = "bed", min_duration_ms = 60000 }
]

# Guard-less transitions (auto-escalation by dwell)
[[fsm.transitions]]
from = "alarmed"
to = "blinded"
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
| `model_not_loaded` | `model` | Referenced model failed to load |

**Dwell semantics:** A guard must evaluate true consistently for `min_duration_ms` before the transition fires. Counter resets on any false evaluation.

---

## Superloop Phases

| Phase | Action | WCET target |
|---|---|---|
| `TIMERS` | Advance `Ton`/`Tof` dwell counters, check stale timers | < 1µs |
| `EVALUATE` | Evaluate FSM guards against current detections + zone state | < 10µs |
| `INGEST` | Read one frame from RTSP (non-blocking try-read), decode if keyframe | < 5ms decode |
| `INFER` | Run models requested by active FSM state. Skip models whose `interval_min_ms` hasn't elapsed. | 20-200ms per model |
| `ZONES` | Map detections to spatial zones, update zone occupancy state | < 10µs |
| `FSM` | Evaluate transitions with satisfied dwells, advance state | < 10µs |
| `PUBLISH` | Flush accumulated events to stdout | < 100µs |

The loop runs **as fast as the slowest phase allows** (bottlenecked by INFER). When no inference is scheduled for a cycle, the loop runs at frame rate (~30-60 fps decode only, no model cost).

## Event Output (JSONL)

Every line is a complete JSON object terminated by `\n`. All timestamps are ISO 8601 with millisecond precision.

```json
{"t":"2026-08-03T20:15:00.123Z","type":"meta","event":"startup","v":"0.1.0"}
{"t":"2026-08-03T20:15:00.200Z","type":"meta","event":"model_loaded","model":"detect-fast","path":"models/yolo26n.onnx","task":"detect","warmup_ms":340}
{"t":"2026-08-03T20:15:00.334Z","type":"health","event":"heartbeat","f":0,"ph":"timers","cyc_us":12}
{"t":"2026-08-03T20:15:00.450Z","type":"frame","f":1,"kf":true,"dec_ms":18}
{"t":"2026-08-03T20:15:00.520Z","type":"detection","f":1,"m":"detect-fast","inf_ms":52,"det":[{"c":"person","conf":0.87,"bb":[100,200,300,500]}]}
{"t":"2026-08-03T20:15:00.530Z","type":"zone","z":"bed","e":"occupied","cls":"person","f":1}
{"t":"2026-08-03T20:15:00.540Z","type":"fsm","from":"idle","to":"monitoring","tr":"bed_occupied","dwell":0}
{"t":"2026-08-03T20:15:02.500Z","type":"fsm","from":"monitoring","to":"alarmed","tr":"bed_exit_attempt","dwell":3000}
{"t":"2026-08-03T20:15:05.000Z","type":"health","event":"stale","c":"ingest","ms":4500}
{"t":"2026-08-03T20:15:10.000Z","type":"health","event":"blind","msg":"No frame for 10000ms"}
{"t":"2026-08-03T20:30:00.000Z","type":"meta","event":"shutdown","reason":"SIGTERM","uptime":900}
```

**Field key abbreviations** (reduce line size for high-throughput):
- `t` = timestamp, `f` = frame, `kf` = keyframe, `dec_ms` = decode_ms
- `m` = model, `inf_ms` = infer_ms, `det` = detections
- `c` = class, `conf` = confidence, `bb` = bbox
- `z` = zone, `e` = event, `cls` = by_class
- `tr` = trigger, `ph` = phase, `cyc_us` = cycle_us
- `v` = version, `c` = component (in health)

## Error Handling

- **Model load failure:** emit `meta/model_load_failed`, skip that model, continue with remaining models. FSM guards referencing failed models are treated as `false`.
- **RTSP disconnect:** emit `health/stale`, enter reconnect loop with exponential backoff (1s, 2s, 4s, 8s, max 30s). During reconnect, FSM evaluates without new frames → will transition to `blind` after `data_stale_ms`.
- **ONNX inference panic:** caught at phase boundary. Increment error counter. If counter exceeds `max_consecutive_panics`, emit `health/blind` and skip that model for N cycles.
- **Guard eval panic:** caught at phase boundary. Treat as `false` for that cycle. Increment error counter.
