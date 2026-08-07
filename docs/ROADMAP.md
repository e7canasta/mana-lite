# Mana Lite Roadmap

*Última actualización: 2026-08-06 — rama `seg-standard` (máscaras + polígonos) y flag `enabled`*

---

## Big Picture

```
 ┌──────────────────────────────────────────────────────────────────────────┐
 │                      MANA LITE — Clinical Perception Pipeline            │
 │                                                                           │
 │  ┌─────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐              │
 │  │ INGEST  │──▶│ DECODE   │──▶│ INFER    │──▶│ TRACK    │              │
 │  │ (done)  │   │ (done)   │   │ 🏗️ S2    │   │ 🏗️ S3    │              │
 │  └─────────┘   └──────────┘   └──────────┘   └──────────┘              │
 │                                    │               │                     │
 │                                    ▼               ▼                     │
 │                              ┌──────────┐   ┌──────────┐                │
 │                              │ ZONES    │──▶│ FSM      │                │
 │                              │ 🏗️ S4    │   │ 🏗️ S4    │                │
 │                              └──────────┘   └──────────┘                │
 │                                                   │                      │
 │      ┌──────────┐   ┌──────────┐   ┌──────────┐  │                      │
 │      │ LOGGER   │   │ HEALTH   │   │ VIZ      │  │                      │
 │      │ (done)   │◀──│ (done)   │◀──│ (done)   │◀─┘                      │
 │      └──────────┘   └──────────┘   └──────────┘                         │
 │           │                                                               │
 │      stdout JSONL  ────▶  clinical consumers / dashboards                │
 └──────────────────────────────────────────────────────────────────────────┘
```

### Leyenda

| Símbolo | Significado |
|---------|------------|
| `done`  | Implementado y con tests |
| `🏗️ S2` | Sprint 2 — actualmente diseñando |
| `🏗️ SX` | Sprint X — planificado |

---

## Estado Actual

**Lo que funciona hoy:**

| Fase | Módulo | Líneas | Tests | Estado |
|------|--------|--------|-------|--------|
| CLI + Config | `main.rs`, `config.rs` | 670 | 5 | ✅ Done |
| Ingesta RTSP | `ingest.rs` (RetinaReader + reconnect) | 406 | 8 | ✅ Done |
| Decode H.264 | `snapshot.rs` (ffmpeg + PNG saver) | 230 | 5 | ✅ Done |
| Cascade | `cascade.rs` (model dependency scheduler) | 130 | 8 | ✅ Done |
| Detection consolidation | `detection.rs` (stateless fusion + enrichment) | 210 | 4 | ✅ Done |
| Tracking | `track.rs` (linear prediction + greedy IoU, optional) | 280 | 10 | 🧪 Prototype |
| Zones | `zones.rs` (ROI evaluation + hysteresis) | 140 | 6 | ✅ Done |
| FSM | `fsm.rs` (clinical state machine) | 320 | 12 | ✅ Done |
| Inferencia | `infer.rs` (ORT session + ultralytics + máscaras CompactMask) | 250 | 10 | ✅ Done |
| Logger JSONL | `logger/` (event, serialize, rotate) | 620 | 8 | ✅ Done |
| Métricas + Health | `metrics.rs` (per-frame class stats + window reports) | 350 | — | ✅ Done |
| Visualización | `viz.rs` (Rerun bridge + blueprint + exponential backoff) | 390 | — | ✅ Done |
| Pipeline State | `pipeline.rs` (superloop orchestrator + frame gap) | 190 | — | ✅ Done |
| **Total** | **12 módulos** | **~3,700** | **58** | |

**Rama `seg-standard` (v0.3):**

- Nueva crate `std/mana-geometry` (compact_mask, polygon, primitives, transform, bbox, iou, polygonize) importada de mana-os (ADR-019) — 154 tests.
- Flag `enabled` por modelo en `config/models.toml` (ADR-020); ejemplo con face/seg apagados en `config/models.example.toml`.
- `seg-standard` como tercer hermano del cascade: `same_frame`, crop `largest_class` persona (ADR-021, Spec-002).
- Máscaras en wire JSONL (rle + bbox + origin + mask_dims + polígonos frame-normalizados) — Spec-003; tests round-trip.
- Overlay RGBA + contornos en Rerun (`log_model_masks`, ADR-022).
- Especificaciones: `docs/specs/seg-standard.md`, `docs/specs/mask-jsonl.md`; sprint en `docs/sprints/seg-standard.md`.

**Observability features (nuevo en v0.1.2):**

- `config/metrics.toml` — text log verbosity + JSONL event toggles
- `config/viz.toml` — 17 Rerun send toggles (per-frame + per-window)
- `config/rerun.toml` — declarative blueprint layout reference
- Per-frame per-class stats: count, conf min/max, area min/max → Rerun time series
- Keyframe gap tracking (`/ingest/normal/gap_ms`) for stream continuity
- Infer latency min-max in text log (not misleading averages)
- Exponential backoff reconnection to Rerun (1s → 30s)
- Hardware device selection (`InferenceConfig::with_device`)

**Lo que está diseñado pero no implementado:**

- `CycleContext` arena (ADR-009)
- `PhaseOutcome` enum (ADR-009)
- `ModelRunner` trait (ADR-009)
- Serde migration (ADR-009 — deferred to v0.2)
- Sistema de errores tipados (`error.rs` — parcialmente)

---

## Sprints y Fases

Las fases 2 y 3 de abajo conservan el diseño objetivo original. El runtime
actual ya ejecuta inferencia y consolidación; el tracker implementado todavía
no es el SORT completo descrito en ADR-013. El estado operativo y los contratos
vigentes están en [ARCHITECTURE.md](ARCHITECTURE.md) y ADR-018.

### 🏗️ Sprint 2 — Inference Core  *(~1 semana)*

> **Objetivo:** Preprocesar frames, correr modelos ONNX, obtener detecciones estructuradas.

```
 FrameBuffer ──▶ Preprocess ──▶ ONNX Session ──▶ Postprocess ──▶ Vec<Detection>
                  (cache)         (per model)       (NMS + scale)
```

**Archivos nuevos:**

| Archivo | Rol | ~Líneas |
|---------|-----|---------|
| `src/infer.rs` | Ejecucion de modelos, filtros por modelo, NMS y escalado | 180 |
| `src/detection.rs` | Consolidacion espacial cross-modelo | 210 |

**ADR relacionados:** 010, 011, 012

**Tests:** Stub ONNX session (sin GPU), preprocess cache hit/miss, NMS con bboxes solapadas.

**Definition of Done:**
- `cargo test -p mana-lite -- infer postprocess` pasa
- Demo mode genera `{"type":"detection","model":"detect-fast","f":1,...}`
- Rerun muestra bounding boxes sobre la cámara

**Riesgos:**
- Compatibilidad ORT API (2.0.0-rc.12 — puede cambiar)
- Rendimiento de resize en CPU para 640×640 (target: <2ms)
- Postprocesadores heredados de mana-inference (¿están actualizados?)

---

### 🏗️ Sprint 3 — Tracking  *(~1 semana)*

> **Objetivo:** Identidad temporal — el mismo objeto a través de frames.

```
 Vec<Detection> ──▶ Predict ──▶ Match (Hungarian) ──▶ Update ──▶ Active Tracks
  (frame N)        (Kalman)     (IoU matrix)         (Kalman)    HashMap<u64, Track>
```

**Archivos nuevos:**

| Archivo | Rol | ~Líneas |
|---------|-----|---------|
| `src/track.rs` | SORT tracker (Kalman 7D, Hungarian, track lifecycle) | 280 |

**ADR relacionados:** 013

**Tests:** Creación de track nuevo, matching con IoU=0.9, eliminación por edad, fuzzy matching (oclusión parcial).

**Definition of Done:**
- `cargo test -p mana-lite -- track` pasa
- JSONL emite `{"type":"track","event":"created","track_id":1,"class":"person"}`
- Rerun muestra tracks con IDs consistentes entre frames

**Riesgos:**
- Parámetros de Kalman necesitan tuning para movimiento clínico (lento, estático por minutos)
- Hungarian es O(n³) — con n=20 tracks es <0.5ms, aceptable
- Caso borde: paciente acostado inmóvil — el track debe persistir sin drift

---

### 🏗️ Sprint 4 — Zones + FSM  *(~1 semana)*

> **Objetivo:** Evaluación espacial y máquina de estados clínica.

```
 Active Tracks ──▶ Zone Engine ──▶ FSM Engine ──▶ Clinical Events
                   (intersect)     (guards+dwell)   (bed_alert, blind...)
```

**Archivos nuevos:**

| Archivo | Rol | ~Líneas |
|---------|-----|---------|
| `src/zones.rs` | Evaluación bbox↔zona, histéresis, dwell counters | 150 |
| `src/fsm.rs` | Evaluador de guards, timers Ton/Tof, transiciones | 220 |

**ADR relacionados:** 014, 015

**Tests:** Zona ocupada/vacía con histéresis, FSM idle→watching por zone_occupied, dwell timer triggering bed_alert, wildcard data_stale → blind.

**Definition of Done:**
- FSM idle→watching cuando persona en zona "bed" por >500ms
- FSM watching→bed_alert cuando zona "bed" vacía por >3s
- FSM *→blind cuando data_stale por >10s
- `{"type":"fsm","from":"watching","to":"bed_alert","trigger":"bed_vacated","dwell_ms":3000}`

**Riesgos:**
- Precisión temporal de dwell timers (miden `Instant::elapsed()`, no timestamps de cámara)
- Histéresis de zona: evitar oscilación ocupado/vacío (500ms default, configurable)
- FSM guards con múltiples condiciones (AND lógico, no OR)

---

### 🏗️ Sprint 5 — Cascade + Integration  *(~4 días)*

> **Objetivo:** Scheduler de modelos lazy + integración end-to-end.

```
 FSM state ──▶ Cascade Scheduler ──▶ modelos a ejecutar
               (interval+requires)   [detect, pose, face...]
```

**Archivos nuevos:**

| Archivo | Rol | ~Líneas |
|---------|-----|---------|
| `src/cascade.rs` | Lazy scheduler: interval + requires + scope | 160 |

**ADR relacionados:** 016

**Work existente adaptado:**
- Extender `main.rs` `superloop` con las fases INFER → TRACK → ZONES → FSM
- `CycleContext` arena (ADR-009)
- `PhaseOutcome` enum para ghost mode

**Tests:** Integration test: frame sintético → detect → track → zone → FSM transition (end-to-end sin cámara real).

**Definition of Done:**
- Pipeline completo corre con `cargo run -- --config config/mana.toml` sobre RTSP real
- Rerun muestra: cámara + bounding boxes + tracks + zonas + texto de eventos
- JSONL contiene todos los tipos de eventos (frame, detection, track, zone, fsm, health, metrics)
- 26 tests existentes siguen pasando + nuevos tests de integración

---

### 🏗️ Sprint 6 — Detection Consolidation  *(~5-7 días)*

> **Objetivo:** consolidar salidas de múltiples modelos en observaciones únicas,
> mantener evidencias multi-rate y publicar una escena sin bbox duplicados.

```
Vec<Detection> ──▶ DetectionConsolidator ──▶ ConsolidatedObservation
                                           │
                                           ▼
                                       Tracker
                                           │
                                           ▼
                                       TrackedEntity
```

**ADR relacionado:** [017](adrs/017-detection-consolidation.md)

**Contrato de etapas:** [018](adrs/018-runtime-stage-boundaries.md)

**Guía del sprint:** [detection-consolidation.md](sprints/detection-consolidation.md)

**Definition of Done:**

- Una persona detectada por detect, pose y face produce una observación consolidada.
- `track_id` pertenece solo a la entidad trackeada, no a la observación ni a cada modelo.
- Rerun muestra un bbox canónico y capas separadas para enriquecimientos.
- JSONL conserva detecciones de diagnóstico y eventos de entidad.
- Pose y face llegan a frecuencias distintas sin crear duplicados.
- Tests de fusión, containment y asociación multi-persona pasan; TTL queda
  diferido al tracking.

---

## ADRs — Architecture Decision Records

| ADR | Tema | Estado |
|-----|------|--------|
| [001](adrs/001-single-binary.md) | Single binary architecture | ✅ Accepted |
| [002](adrs/002-toml-catalog-pattern.md) | TOML catalog pattern | ✅ Accepted |
| [003](adrs/003-plc-superloop.md) | PLC superloop execution model | ✅ Accepted |
| 004 | Retina for RTSP ingest | ✅ Accepted |
| [005](adrs/005-cascaded-inference.md) | Cascaded inference | ✅ Accepted |
| 006 | JSON Lines stdout | ✅ Accepted |
| [007](adrs/007-iframe-gating.md) | I-frame gating | ✅ Accepted |
| [008](adrs/008-ingest-engine.md) | Ingest engine design | ✅ Accepted |
| [009](adrs/009-pipeline-design.md) | Pipeline design (arena, coupling, tests) | ✅ Accepted |
| [010](adrs/010-preprocess-cache.md) | Preprocess tensor cache | 🏗️ Draft |
| [011](adrs/011-inference-engine.md) | Inference engine (ORT pool) | 🏗️ Draft |
| [012](adrs/012-postprocess-pipeline.md) | Postprocess (NMS, unified Detection) | 🏗️ Draft |
| [013](adrs/013-sort-tracking.md) | SORT tracking for clinical scenes | 🏗️ Draft |
| [014](adrs/014-zone-engine.md) | Zone engine (spatial + hysteresis) | 🏗️ Draft |
| [015](adrs/015-fsm-engine.md) | FSM engine (guards, dwell, Ton/Tof) | 🏗️ Draft |
| [016](adrs/016-cascade-scheduler.md) | Cascade scheduler (interval + requires) | 🏗️ Draft |
| [017](adrs/017-detection-consolidation.md) | Detection consolidation across models | ✅ Accepted |
| [018](adrs/018-runtime-stage-boundaries.md) | Runtime stage and publication boundaries | ✅ Accepted |
| [019](adrs/019-import-mana-os-std.md) | Import de std de mana-os (copia de crates) | ✅ Accepted |
| [020](adrs/020-model-enabled-flag.md) | Flag `enabled` por modelo en models.toml | ✅ Accepted |
| [021](adrs/021-mask-format-compactmask.md) | Formato de máscara (CompactMask + polígonos) | ✅ Accepted |
| [022](adrs/022-mask-overlay-rgba.md) | Overlay de máscaras RGBA en Rerun | ✅ Accepted |

---

## Timeline

```
 Semana 1         Semana 2         Semana 3         Semana 4
 ┌────────────────┬────────────────┬────────────────┬────────────────┐
 │ Sprint 2       │ Sprint 3       │ Sprint 4       │ Sprint 5       │
 │ INFER core     │ TRACKING       │ ZONES + FSM    │ CASCADE + INT  │
 │                │                │                │                │
 │ preprocess.rs  │ track.rs       │ zones.rs       │ cascade.rs     │
 │ infer.rs       │ Kalman 7D      │ fsm.rs         │ CycleContext   │
 │ postprocess.rs │ Hungarian      │ dwell timers   │ PhaseOutcome   │
 │                │                │ Ton/Tof        │ integration    │
 └────────────────┴────────────────┴────────────────┴────────────────┘
                                     │
                                     ▼
                               v0.2.0 RELEASE
                          Full clinical pipeline
```

### Hitos

| Hito | Condición | Validación |
|------|-----------|-----------|
| **S2 done** | `detect-fast` corre en CPU, genera detecciones | `cargo test` + RTSP real con bboxes en Rerun |
| **S3 done** | Tracks persisten entre frames con IDs consistentes | `cargo test` + video de 30s con el mismo track ID |
| **S4 done** | FSM idle→watching→bed_alert→blind con zona real | `cargo test` + `config/fsm.toml` completo evaluado |
| **S5 done** | Pipeline end-to-end con RTSP | `cargo run -- --config config/mana.toml` produce JSONL completo |
| **Release** | 26 tests existentes + nuevos pasan, clippy limpio | CI verde, binario < 20MB |

---

## Deuda Técnica (post-v0.2)

| Item | Prioridad | ADR |
|------|-----------|-----|
| Migrar serializador a serde | Baja | 009 |
| `CycleContext` arena con `clear()` | Media | 009 |
| Sistema de errores tipados completo | Media | — |
| Remover `mana-rtsp` crate (1 fn) → merge a ingest | Baja | — |
| VAAPI/NVDec hardware decode | Baja | — |
| `mana-lite replay --mp4` | Baja | — |
| Zenoh bridge para interop con full Mana OS | Baja | — |
