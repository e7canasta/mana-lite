# Mana Lite Architecture

## Big Picture

Mana Lite is a **single-binary clinical perception pipeline** that transforms an RTSP video stream into structured clinical events. It operates as a deterministic **PLC-style superloop**: one thread, seven phases, every cycle.

```
 ┌────────────────────────────────────────────────────────────────────┐
 │                        Mana Lite (1 process)                       │
 │                                                                    │
 │   ┌──────────────────────────────────────────────────────────┐    │
 │   │                    Config Catalog                         │    │
 │   │  mana.toml  models.toml  zones.toml  fsm.toml            │    │
 │   └──────────────────────────────────────────────────────────┘    │
 │                              │                                     │
 │   ┌──────────────────────────▼────────────────────────────────┐   │
 │   │                     SUPERLOOP (7 phases)                   │   │
 │   │                                                            │   │
 │   │  [TIMERS] → [EVALUATE] → [INGEST] → [INFER] →            │   │
 │   │  [ZONES] → [FSM] → [PUBLISH]                              │   │
 │   │                                                            │   │
 │   │  single-thread, deterministic, no alloc in hot path       │   │
 │   └──────────────────────────────────────────────────────────┘   │
 │                              │                                     │
 │                    stdout JSONL (one line per event)               │
 └────────────────────────────────────────────────────────────────────┘
```

## Design Principles

1. **Delete > Replace > Add.** The inference codebase already has 80% of what we need. Mana Lite strips it down and wraps it.

2. **Mechanism ≠ Policy ≠ State (ADR-005).** Keeping these separate:
   - **Mechanism:** retina RTSP client, ORT inference, iou/nms math
   - **Policy:** `models.toml` thresholds, `zones.toml` regions, `fsm.toml` guards
   - **State:** current FSM state, last detections, dwell counters — runtime only

3. **Superloop, not async.** No tokio spawns, no channels, no callbacks. The main loop reads one frame, processes it completely, publishes, repeats. WCET is predictable.

4. **Catalog over CLI.** Runtime behavior is defined in TOML files, not command-line flags. `mana-lite --config mana.toml` is the only required argument.

5. **Zero-copy where it matters.** Preprocessed tensors are fed directly to ORT without intermediate allocations. Detections flow by reference until serialization.

## Data Flow

```
 Camera (RTSP)
      │
      ▼
 retina::Session
      │  H.264 access units (encoded)
      ▼
 ffmpeg h264 decode
      │  RGB pixel buffer
      ▼
 i-frame gate ──── non-keyframe → skip (emit ghost if configured)
      │
      ▼
 ONNX Runtime
      │  raw tensors (float32)
      ▼
 postprocess (decode + NMS)
      │  Vec<Detection>
      ▼
 ZoneEngine
      │  Vec<ZoneChange>
      ▼
 FsmEngine
      │  Option<FsmTransition>
      ▼
 Logger::flush()
      │  stdout (JSONL)
```

## Module Map

```
src/
├── main.rs          Entry point, config loading, superloop orchestration
├── config.rs        Parsing for all four TOML schemas
├── ingest.rs        Retina RTSP client + ffmpeg decode + keyframe detection
├── infer.rs         ONNX Runtime wrapper (per-model session management)
├── cascade.rs       Multi-model execution scheduling with timers
├── zones.rs         Detection-to-zone spatial evaluation
├── fsm.rs           Hierarchical state machine engine
├── logger.rs        Buffered JSONL event emitter
└── error.rs         Error types and Result alias
```

## Dependency Graph

```
main.rs
 ├── config.rs (serde, toml)
 ├── ingest.rs (retina + ffmpeg-next)
 ├── infer.rs  (ort)
 ├── cascade.rs (depends on infer.rs, config.rs)
 ├── zones.rs  (pure math, no deps)
 ├── fsm.rs    (depends on zones.rs, config.rs)
 ├── logger.rs (serde_json)
 └── error.rs  (thiserror)
```

## Comparison: Mana Lite vs Full Mana OS

| Dimension | Full Mana OS | Mana Lite |
|---|---|---|
| Processes | 8+ (iceoryx2 SHM) | 1 |
| IPC | iceoryx2 pub/sub | in-memory |
| Control plane | Zenoh | stdout JSONL |
| Launch | Topological DAG (Kahn's algorithm) | `cargo run` |
| Tracking | Kalman (mana-track) | Per-frame only |
| World model | Retained state (mana-world) | Transient |
| Clinical reasoning | BrainService FSM | Embedded FSM |
| Deployment | System-wide daemons | systemd unit |
| Config | Rust structs + CLI | TOML files |
| Code size | 50K+ LOC across 15+ crates | < 3K LOC |
