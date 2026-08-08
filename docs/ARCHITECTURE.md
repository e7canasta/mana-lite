# Mana Lite Architecture

*Última actualización: 2026-08-07 — depth ROI-local + pose + máscaras*

---

## Big Picture

```
 ┌──────────────────────────────────────────────────────────────────────┐
 │                    Mana Lite — Clinical Perception Pipeline          │
 │                                                                       │
 │  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐        │
 │  │INGEST  │─▶│DECODE  │─▶│INFER   │─▶│CONSOL. │─▶│PUBLISH │        │
 │  │ ✅      │  │ ✅      │  │ ✅      │  │ ✅      │  │ ✅      │        │
 │  └────────┘  └────────┘  └────────┘  └────────┘  └────┬───┘        │
 │                                                        │             │
 │                        ┌────────┐  ┌────────┐  ┌──────▼───┐        │
 │                        │ TRACK  │─▶│  FSM   │◀─│  ZONES   │        │
 │                        │ optional│  │ optional│  │ optional│        │
 │                        └────────┘  └────────┘  └──────────┘        │
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

3. **Catalog over CLI.** Comportamiento definido en TOML, no en flags. `mana-lite --config mana.toml` es el único argumento requerido. El perfil de inferencia se selecciona con `inference.blueprint_file`. [ADR-002](adrs/002-toml-catalog-pattern.md), [ADR-026](adrs/026-inference-blueprints.md)

4. **Explicit ownership per cycle.** `main.rs` owns the current frame and pending model outputs; consolidation borrows detection slices and returns only the evidence it needs. The planned `CycleContext` arena is not part of the current implementation. [ADR-018](adrs/018-runtime-stage-boundaries.md)

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
       │                        ──▶ crop depth 680x680 ───▶ depth-standard
       ▼
  InferEngine (ORT session pool)
       │  Vec<ort::Value> (tensores crudos)
       ▼
   InferEngine (NMS + filtros por modelo + escala)
        │  Vec<Detection> { class, conf, bbox, keypoints, mask }
        │  DepthMap { data: Array2<f32> }  (local al ROI, ADR-024)
        │
        ▼
   DetectionConsolidator (fusion + enrichment, stateless)
        │  ConsolidatedObservation { bbox canónico, evidence, components }
        ▼
   PresenceFilter (optional signal debounce + short dropout hold)
        │
        ▼
   ┌──────────────────────────────────────────────────────────────┐
  │ tracking disabled: JSONL consolidated_detection + Rerun      │
  │ /world/camera/observations                                   │
  └──────────────────────────────────────────────────────────────┘
        │ tracking enabled only
        ▼
  TrackingEngine (TrackedEntity: identity + freshness)
       │  HashMap<u64, TrackState>
       │  TrackEvent { Created, Updated, Lost, Deleted }
       │
       ├─▶ CascadeScheduler ──▶ semantic gates + child model crop
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
  │  Event::Frame, Detection, ConsolidatedDetection, Entity, │
  │  Meta(track_*), Zone, FSM, Health, Metrics      │
 └─────────────────────────────────────────────┘
      │
      ▼
 VizBridge (Rerun gRPC)
```

## Module Map (current)

```
src/
├── main.rs               Entry point, superloop orchestration ✅
├── config.rs             Parsing for all TOML schemas         ✅
├── ingest.rs             Retina RTSP + keyframe drain + reconnect ✅
├── snapshot.rs           H.264 decode + RGB buffer + PNG saver ✅
├── infer.rs              Model execution + filters + NMS + masks ✅
├── detection.rs          Stateless cross-model consolidation       ✅
├── presence.rs           Temporal presence/signal debounce          ✅
├── track.rs              Linear prediction + greedy IoU tracker    🧪 optional
├── zones.rs              Spatial zone evaluation + hysteresis      ✅
├── fsm.rs                Clinical FSM engine + guard evaluation    ✅
├── cascade.rs            Model scheduler + track crop eligibility  ✅
├── pipeline.rs           PipelineState runtime                    ✅
├── metrics.rs            MetricsEngine + Health + PerClassFrameStats ✅
├── viz.rs                VizBridge + Rerun blueprint              ✅
├── logger/
│   ├── mod.rs            Buffered JSONL emitter + file rotation   ✅
│   ├── event.rs          Event type definitions                   ✅
│   └── serialize.rs      Manual JSON serializer                   ✅
└── error.rs              Typed error enums                        ✅
```

### Config TOML files

| File | Loaded via | Purpose |
|---|---|---|
| `config/mana.toml` | `load_app_config()` | Top-level: stream source, pipeline toggles, output paths |
| `config/models.toml` | `load_model_catalog()` | ONNX model catalog: paths, tasks, imgsz, confidence, `enabled` flag (ADR-020), per-model crop ROI |
| `config/models.example.toml` | — | Ejemplo: ramas face/seg con `enabled = false` |
| `config/cascade.toml` | `load_config::<CascadeConfig>()` | Model dependency graph (requires, requires_class) |
| `config/blueprints/<name>/blueprint.toml` | `load_config::<BlueprintConfig>()` | Named model set, primary root and cascade rules |
| `config/fsm.toml` | `load_fsm_catalog()` | Clinical state machine: states, models, transitions |
| `config/zones.toml` | `load_zone_catalog()` | Spatial ROIs for tracking + FSM zone guards |
| `config/metrics.toml` | `load_metrics_log()` | Text log verbosity + metrics settings |
| `config/viz.toml` | `load_viz_data()` | Rerun send toggles (per-frame + per-window channels) |
| `config/rerun.toml` | `load_rerun_blueprint()` | Rerun viewer blueprint layout |

## Dependency Graph

```
main.rs
 ├── config.rs ────────────── serde, toml
 ├── ingest.rs ────────────── retina, mana-rtsp, url
 │    └── RetinaReader (async RTSP + reconnect)
 ├── snapshot.rs ──────────── ffmpeg-next, image, mana-video
 │    └── FrameDecoder, SnapshotSaver
  ├── infer.rs ─────────────── ultralytics inference + NMS      ✅
  │    └── InferEngine
  ├── detection.rs ─────────── pure spatial consolidation        ✅
  │    └── DetectionConsolidator
  ├── track.rs ─────────────── linear prediction + greedy IoU    🧪 optional
  │    └── Tracker
  ├── zones.rs ──────────────── (pure math: AABB intersection)   ✅ optional
 │    └── ZoneEngine
  ├── fsm.rs ────────────────── config::FsmCatalog               ✅ optional
 │    └── FsmEngine
  ├── cascade.rs ────────────── config::ModelCatalog             ✅
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

## Cascade model branches

The selected blueprint is the deployment boundary for the inference graph. The
legacy standalone `cascade.toml` remains supported when no blueprint is
selected, but a selected blueprint takes precedence over it.

El cascade (`src/cascade.rs`) programa los modelos en topo-orden; cada modelo puede
declarar `requires` + `requires_class` en `config/cascade.toml`. La topología actual:

```
detect-fast (root, siempre corre)
  ├── pose-standard   (requires=detect-fast, requires_class=person, same_frame,
  │                    crop largest_class)
  ├── face-yolo       (requires=detect-fast, requires_class=person,
  │                    requires_exact_count=1, same_frame, crop square upper-body;
  │                    el ROI hijo puede exceder el ROI del padre — ADR-023)
  └── seg-standard    (requires=detect-fast, requires_class=person, same_frame=true,
                       crop largest_class margin 0.15)   ← rama v0.3 (ADR-019..022)
depth-standard (root independiente, sin requires, crop static [560,140 1240,820])
                       ← rama v0.4 (ADR-024, docs/specs/depth-standard.md)
```

`depth-standard` corre siempre sobre su ROI fijo, produce un mapa local 680x680,
publica estadísticas y no entra en consolidación, tracking, zonas ni FSM.


Ramas hermanas: `seg-standard` no depende de pose/face y viceversa — si una falla,
las otras siguen. Tres formas de excluir un modelo del ciclo:

1. **`enabled = false`** en `[models.<key>]` (ADR-020, default `true`): el modelo se
   filtra en `App::resolve_models` antes del cascade; no se programa ni se infiere.
   Ver ejemplo en `config/models.example.toml`.
2. **FSM**: si el estado actual no lista el modelo, no entra en `ordered()`.
3. **Cascade**: sin track confirmado de la clase del padre, `should_run` es false.

Salida de `seg-standard`: bboxes + máscaras `CompactMask` (crop-RLE, `vernier-mask`)
+ polígonos de contorno simplificados (RDP 0.75), ambos derivados del crop y
normalizados al frame. Wire JSONL y overlay Rerun: Spec-003 y ADR-022.

## Cycle Lifecycle (Superloop)

```
┌──────────────────────────────────────────────────────┐
│                    SUPERLOOP CYCLE                    │
│                                                       │
│  begin cycle; keep owned frame and pending outputs    │
│                                                       │
│  ┌─ PHASE 0: TIMERS ──────────────────────────┐      │
│  │ cascade.advance(); health.tick();           │      │
│  │ < 1µs                                       │      │
│  └──────────────────────────────────────────────┘      │
│                         │                              │
│  ┌─ PHASE 1: INGEST ──────────────────────────┐      │
│  │ kf = ingest.poll_freshest_keyframe().await; │      │
│  │ fb = decoder.decode_timed(&kf.h264);        │      │
│  │ frame = fb;  // owned current-frame buffer  │      │
│  │ < 5ms (decode)                              │      │
│  └──────────────────────────────────────────────┘      │
│                         │                              │
│                    ┌────▼──── no frame? skip INFER     │
│                    │                                   │
│  ┌─ PHASE 2: INFER ──────────────────────────┐       │
│  │ run eligible models;                         │       │
│  │ postprocess per-model detections;            │       │
│  │ consolidate(model_detections);               │       │
│  │ publish consolidated observations;           │       │
│  │ if tracking: update tracks/entities;         │       │
│  │ if tracks exist: run eligible child models;  │       │
│  │ < 200ms (3 modelos CPU)                     │       │
│  └──────────────────────────────────────────────┘       │
│                         │                              │
│  ┌─ PHASE 3: OPTIONAL TRACK / SCENE ───────────┐      │
│  │ entity events only when tracking is enabled;│      │
│  │ observations remain frame-local;            │      │
│  │ < 1ms (20 tracks)                           │      │
│  └──────────────────────────────────────────────┘      │
│                         │                              │
│  ┌─ PHASE 4: ZONES ───────────────────────────┐      │
│  │ events = zones.evaluate(&tracks);            │      │
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
│  │ snapshots.save(frame);  // PNG + H.264       │      │
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
| Tracking | Kalman (mana-track) | Linear prediction + greedy IoU, optional |
| World model | Retained state (mana-world) | Transient per-cycle |
| Clinical reasoning | BrainService FSM | Embedded FSM |
| Deployment | System-wide daemons | systemd unit |
| Config | Rust structs + CLI | TOML files |
| Code size | 50K+ LOC across 15+ crates | ~5K LOC (v0.2.0 target) |
| GPU | Required (CUDA/ROCm) | Optional (CPU-first) |
