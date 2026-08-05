# Mana Lite Architecture

*Última actualización: 2026-08-04 — post-refactor, pre-inference*

---

## Big Picture

```
 ┌──────────────────────────────────────────────────────────────────────┐
 │                    Mana Lite — Clinical Perception Pipeline          │
 │                                                                       │
 │  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐        │
 │  │INGEST  │─▶│DECODE  │─▶│INFER   │─▶│TRACK   │─▶│ZONES   │        │
 │  │ ✅ 1.0 │  │ ✅ 1.0 │  │ 🏗️ S2  │  │ 🏗️ S3  │  │ 🏗️ S4  │        │
 │  └────────┘  └────────┘  └────────┘  └────────┘  └────┬───┘        │
 │                                                        │             │
 │                        ┌────────┐  ┌────────┐  ┌──────▼───┐        │
 │                        │PUBLISH │◀─│  FSM   │◀─│ CASCADE  │        │
 │                        │ ✅ 1.0 │  │ 🏗️ S4  │  │ 🏗️ S5   │        │
 │                        └────┬───┘  └────────┘  └──────────┘        │
 │                             │                                        │
 │                        stdout JSONL                                   │
 │                             │                                        │
 │              ┌──────────────┼──────────────┐                         │
 │              ▼              ▼              ▼                          │
 │         clinical      dashboard      alerting                        │
 │         consumer      (Rerun)        system                          │
 └──────────────────────────────────────────────────────────────────────┘
```

## Design Principles

1. **Single binary, single thread, PLC superloop.** Un proceso, un hilo, siete fases secuenciales. Sin channels, sin spawn, sin IPC. [ADR-001](adrs/001-single-binary.md), [ADR-003](adrs/003-plc-superloop.md)

2. **Mechanism ≠ Policy ≠ State.** Retina RTSP, ORT inference, y NMS math son *mechanism*. `models.toml`, `zones.toml`, `fsm.toml` son *policy*. `PipelineState`, `TrackState`, `FsmEngine.current_state` son *state*. [ADR-002](adrs/002-toml-catalog-pattern.md)

3. **Catalog over CLI.** Comportamiento definido en TOML, no en flags. `mana-lite --config mana.toml` es el único argumento requerido. [ADR-002](adrs/002-toml-catalog-pattern.md)

4. **Arena allocation per cycle.** `CycleContext` aloja todos los datos intermedios una vez por ciclo. Cada fase toma `&mut CycleContext`, llena su slot, retorna. Al final: `clear()`, sin dealloc. [ADR-009](adrs/009-pipeline-design.md)

5. **Generics for testability.** Cada engine es genérico sobre su dependencia externa (`FrameReader`, `ModelRunner`). Tests usan stubs. Producción usa implementaciones reales. Static dispatch — sin vtable. [ADR-009](adrs/009-pipeline-design.md)

6. **Two-layer logging.** `log::info!` para operadores (stderr). `Logger::emit()` para sistemas downstream (stdout JSONL). Nunca mezclar. [ADR-009](adrs/009-pipeline-design.md)

## Data Flow (Completo — v0.2.0 target)

```
 Camera (RTSP)                                    ┌─────────────────────┐
      │                                            │  CONFIG CATALOG     │
      ▼                                            │  models.toml        │
 RetinaReader                                     │  zones.toml         │
      │  H.264 Annex-B (encoded)                   │  fsm.toml           │
      ▼                                            └─────────────────────┘
 RetinaReader.next_frame()
      │  keyframe_only + dedup                        ┌──────────────┐
      ▼                                               │ HEALTH        │
 FrameDecoder (ffmpeg)                                │ blind/stale/  │
      │  YUV → RGB24 packed                           │ recovered     │
      ▼                                               └──────────────┘
 FrameBuffer { w, h, rgb }
      │
      ├─▶ PreprocessCache  ──▶ imgsz=320 tensor  ──▶ detect-fast
      │   (cache por imgsz)  ──▶ imgsz=640 tensor  ──▶ pose-standard
      │                                                   face-v12
      │
      ▼
 InferEngine (ORT session pool)
      │  Vec<ort::Value> (tensores crudos)
      ▼
 Postprocessor (NMS + escala + keypoints)
      │  Vec<Detection> { class, conf, bbox, keypoints, mask }
      │
      ├─▶ CascadeScheduler ──▶ decide qué modelos correr próximo ciclo
      │
      ▼
 TrackingEngine (SORT: Kalman 7D + Hungarian)
      │  HashMap<u64, TrackState>
      │  TrackEvent { Created, Updated, Lost, Deleted }
      │
      ▼
 ZoneEngine (intersección + histéresis)
      │  ZoneEvent { Occupied, Vacated }
      │
      ▼
 FsmEngine (guards + dwell timers)
      │  FsmTransition { from, to, trigger, dwell_ms }
      │
      ▼
 ┌─────────────────────────────────────────────┐
 │              Logger (JSONL stdout)           │
 │  Event::Frame, Detection, Track, Zone, FSM, │
 │  Health, Metrics, Meta                       │
 └─────────────────────────────────────────────┘
      │
      ▼
 VizBridge (Rerun gRPC)
```

## Module Map (v0.2.0 target)

```
src/
├── main.rs               Entry point, superloop orchestration ✅
├── config.rs             Parsing for all seven TOML schemas ✅
├── ingest.rs             Retina RTSP + keyframe drain + reconnect ✅
├── snapshot.rs           H.264 decode + RGB buffer + PNG saver ✅
├── preprocess.rs         Letterbox resize + tensor cache          🏗️ S2
├── infer.rs              ORT session pool + warmup + dispatch     🏗️ S2
├── postprocess.rs        NMS + unified Detection type             🏗️ S2
├── track.rs              SORT tracker (Kalman + Hungarian)        🏗️ S3
├── zones.rs              Spatial zone evaluation + hysteresis     🏗️ S4
├── fsm.rs                Clinical FSM engine + guard evaluation   🏗️ S4
├── cascade.rs            Lazy model scheduler                     🏗️ S5
├── pipeline.rs           PipelineState runtime                    ✅
├── metrics.rs            MetricsEngine + Health + PerClassFrameStats ✅
├── viz.rs                VizBridge + Rerun blueprint              ✅
├── logger/
│   ├── mod.rs            Buffered JSONL emitter + file rotation   ✅
│   ├── event.rs          Event type definitions                   ✅
│   └── serialize.rs      Manual JSON serializer                   ✅
└── error.rs              Typed error enums                        ✅
```

### Config TOML files (all seven)

| File | Loaded via | Purpose |
|---|---|---|
| `config/mana.toml` | `load_app_config()` | Top-level: stream source, pipeline toggles, output paths |
| `config/models.toml` | `load_model_catalog()` | ONNX model catalog: paths, tasks, imgsz, confidence |
| `config/cascade.toml` | `load_config::<CascadeConfig>()` | Model dependency graph (requires, requires_class) |
| `config/fsm.toml` | `load_fsm_catalog()` | Clinical state machine: states, models, transitions |
| `config/zones.toml` | `load_zone_catalog()` | Spatial ROIs for tracking + FSM zone guards |
| `config/metrics.toml` | `load_metrics_log()` | Text log verbosity + JSONL event toggles |
| `config/viz.toml` | `load_viz_data()` | Rerun send toggles (per-frame + per-window channels) |
| `config/rerun.toml` | `load_rerun_blueprint()` | Rerun viewer blueprint layout (declarative reference) |

## Dependency Graph

```
main.rs
 ├── config.rs ────────────── serde, toml
 ├── ingest.rs ────────────── retina, mana-rtsp, url
 │    └── RetinaReader (async RTSP + reconnect)
 ├── snapshot.rs ──────────── ffmpeg-next, image, mana-video
 │    └── FrameDecoder, SnapshotSaver
 ├── preprocess.rs ────────── image (resize), ort::Tensor      🏗️
 │    └── PreprocessCache
 ├── infer.rs ─────────────── ort (ONNX Runtime)                🏗️
 │    └── InferEngine (session pool)
 ├── postprocess.rs ────────── (pure math: NMS, scale, IoU)     🏗️
 │    └── DetectPostprocessor, PosePostprocessor, ...
 ├── track.rs ─────────────── nalgebra (Kalman), (Hungarian)    🏗️
 │    └── TrackingEngine
 ├── zones.rs ──────────────── (pure math: AABB intersection)   🏗️
 │    └── ZoneEngine
 ├── fsm.rs ────────────────── config::FsmCatalog               🏗️
 │    └── FsmEngine
 ├── cascade.rs ────────────── config::ModelCatalog             🏗️
 │    └── CascadeScheduler
 ├── pipeline.rs ──────────── logger, metrics, health            ✅
 │    └── PipelineState
  ├── metrics.rs ────────────── (pure Rust: counters + timers + per-frame class stats)    ✅
  │    └── MetricsEngine, Health, MetricsReport, PerClassFrameStats
 ├── viz.rs ────────────────── rerun, mana-viz, mana-types       ✅
 │    └── VizBridge
 ├── logger/ ───────────────── chrono, std::io, std::fs          ✅
 │    └── Logger, Event, serialize
 └── error.rs ──────────────── thiserror                          ✅
     └── ManaError, ConfigError, Result<T>
```

## Cycle Lifecycle (Superloop)

```
┌──────────────────────────────────────────────────────┐
│                    SUPERLOOP CYCLE                    │
│                                                       │
│  ctx.clear();  // reset arena, keep allocations       │
│                                                       │
│  ┌─ PHASE 0: TIMERS ──────────────────────────┐      │
│  │ cascade.advance(); health.tick();           │      │
│  │ < 1µs                                       │      │
│  └──────────────────────────────────────────────┘      │
│                         │                              │
│  ┌─ PHASE 1: INGEST ──────────────────────────┐      │
│  │ kf = ingest.poll_freshest_keyframe().await; │      │
│  │ fb = decoder.decode_timed(&kf.h264);        │      │
│  │ ctx.frame = fb;  // arena slot              │      │
│  │ < 5ms (decode)                              │      │
│  └──────────────────────────────────────────────┘      │
│                         │                              │
│                    ┌────▼──── no frame? skip INFER     │
│                    │                                   │
│  ┌─ PHASE 2: INFER ──────────────────────────┐       │
│  │ models = cascade.schedule(fsm.active());   │       │
│  │ for m in models:                           │       │
│  │   tensor = preprocess.get(m.imgsz, fb);    │       │
│  │   outputs = infer.run(m, tensor);          │       │
│  │   detections = postprocess.(outputs, fb);  │       │
│  │ ctx.detections.extend(detections);         │       │
│  │ < 200ms (3 modelos CPU)                     │       │
│  └──────────────────────────────────────────────┘       │
│                         │                              │
│  ┌─ PHASE 3: TRACK ───────────────────────────┐      │
│  │ events = tracker.update(&ctx.detections);   │      │
│  │ ctx.tracks = tracker.active();              │      │
│  │ log.emit_all(events);                       │      │
│  │ < 1ms (20 tracks)                           │      │
│  └──────────────────────────────────────────────┘      │
│                         │                              │
│  ┌─ PHASE 4: ZONES ───────────────────────────┐      │
│  │ events = zones.evaluate(&ctx.tracks);       │      │
│  │ log.emit_all(events);                       │      │
│  │ < 0.1ms                                     │      │
│  └──────────────────────────────────────────────┘      │
│                         │                              │
│  ┌─ PHASE 5: FSM ─────────────────────────────┐      │
│  │ transition = fsm.evaluate(events, tracks);  │      │
│  │ if let Some(t) = transition {               │      │
│  │     state.transition_to(t.to);              │      │
│  │     log.emit(t);                            │      │
│  │ }                                           │      │
│  │ < 0.1ms                                     │      │
│  └──────────────────────────────────────────────┘      │
│                         │                              │
│  ┌─ PHASE 6: PUBLISH ─────────────────────────┐      │
│  │ log.flush();  // JSONL stdout + file rotate │      │
│  │ viz.tick();   // Rerun flush                │      │
│  │ snapshots.save(ctx.frame);  // PNG + H.264  │      │
│  │ < 1ms (buffer flush)                        │      │
│  └──────────────────────────────────────────────┘      │
│                                                       │
│  ┌─ PHASE 7: HEALTH ──────────────────────────┐      │
│  │ state.evaluate_health(&mut health, log, m); │      │
│  │ < 1µs                                       │      │
│  └──────────────────────────────────────────────┘      │
│                                                       │
│  if state.should_exit() { break; }                     │
└──────────────────────────────────────────────────────┘
```

## Comparison: Mana Lite vs Full Mana OS

| Dimension | Full Mana OS | Mana Lite |
|---|---|---|
| Processes | 8+ (iceoryx2 SHM) | 1 |
| IPC | iceoryx2 pub/sub | in-memory references |
| Control plane | Zenoh | stdout JSONL |
| Launch | Topological DAG | `cargo run -- --config mana.toml` |
| Tracking | Kalman (mana-track) | SORT (embedded) |
| World model | Retained state (mana-world) | Transient per-cycle |
| Clinical reasoning | BrainService FSM | Embedded FSM |
| Deployment | System-wide daemons | systemd unit |
| Config | Rust structs + CLI | TOML files |
| Code size | 50K+ LOC across 15+ crates | ~5K LOC (v0.2.0 target) |
| GPU | Required (CUDA/ROCm) | Optional (CPU-first) |
