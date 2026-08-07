# ADR-006: JSON Lines Output to stdout

**Status:** Accepted
**Date:** 2026-08-03

## Context

Mana Lite needs to emit structured events (detections, zone changes, FSM transitions, health). These events must be consumable by downstream systems: log aggregators, alert pipelines, dashboards, message queues.

Full Mana OS uses Zenoh for the control plane. We explicitly reject that for v0 to keep Mana Lite dependency-free.

## Decision

**JSON Lines (JSONL) to stdout.** One event = one line = one complete JSON object terminated by `\n`.

Events are buffered in-memory during the superloop cycle and flushed atomically in the PUBLISH phase. This prevents partial writes and interleaved output.

### Event Schema

Every event has a `t` (timestamp) and `type` field:

```jsonl
{"t":"2026-08-03T20:15:00.123Z","type":"meta","event":"startup","v":"0.1.0"}
{"t":"2026-08-03T20:15:00.450Z","type":"frame","f":1,"kf":true,"dec_ms":18}
{"t":"2026-08-03T20:15:00.520Z","type":"detection","f":1,"m":"detect-fast","inf_ms":52,"det":[{"c":"person","conf":0.87,"bb":[100,200,300,500]}]}
{"t":"2026-08-03T20:15:00.530Z","type":"zone","z":"bed","e":"occupied","cls":"person","f":1}
{"t":"2026-08-03T20:15:00.540Z","type":"fsm","from":"idle","to":"monitoring","tr":"bed_occupied","dwell":0}
{"t":"2026-08-03T20:15:10.000Z","type":"health","event":"blind","msg":"No frame for 10000ms"}
```

### Event Types

| `type` | Semantic |
|---|---|
| `meta` | Lifecycle: startup, shutdown, model loaded, model failed |
| `health` | Liveness: heartbeat, stale, blind, panic count |
| `frame` | Per-frame: frame number, keyframe, decode time |
| `detection` | Inference: model name, detections array, infer time |
| `consolidated_detection` | Same-frame spatial fusion without temporal identity |
| `entity` | Tracked temporal identity with `track_id` |
| `zone` | Spatial: zone name, occupied/vacated, triggering class |
| `fsm` | Clinical: from/to/trigger/dwell on state transition |

### Logger API

```rust
pub struct Logger {
    buffer: Vec<Event>,
}

impl Logger {
    pub fn emit(&mut self, event: Event);
    pub fn flush(&mut self);
}

pub enum Event {
    Meta { event: MetaKind, detail: String, attrs: Vec<(String, String)> },
    Health { event: HealthKind, frame: u64, cycle_us: u64, msg: Option<String> },
    Frame { frame: u64, keyframe: bool, decode_ms: u64 },
    Detection { frame: u64, model: String, infer_ms: u64, detections: Vec<DetectionRecord> },
    Zone { zone: String, event: ZoneState, by_class: String, frame: u64 },
    Fsm { from: String, to: String, trigger: String, dwell_ms: u64 },
}
```

## Alternatives Considered

### A. Zenoh control plane (like full Mana OS)

**Rejected for v0:** Adds a network dependency and complex configuration. Mana Lite v1.x may add an optional `--zenoh` flag.

### B. Structured binary (Protobuf, FlatBuffers, Avro)

**Rejected:** Requires schema registry, code generation, specialized consumers. JSONL is trivially consumable by `jq`, Python `json.loads()`, logstash, fluentd.

### C. Unix domain socket

**Rejected:** Adds an IPC boundary. If the consumer crashes, the socket buffers fill and the pipeline blocks (or loses events). stdout + pipe to file/tee avoids this.

## Consequences

- **Positive:** Zero-config consumption. `mana-lite | tee events.jsonl` works immediately.
- **Positive:** `jq` filters on the command line: `mana-lite | jq 'select(.type=="fsm")'`
- **Positive:** Atomic flush per cycle means no partial JSON lines even under load.
- **Negative:** No built-in back-pressure. If the consumer can't keep up, events are lost at the pipe buffer. Mitigated by `--output-dir` mode which writes to files directly.
- **Negative:** Timestamp is UTC only. No timezone configuration in v0.
