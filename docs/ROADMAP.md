# Mana Lite Roadmap

*Última actualización: 2026-08-04 — post-refactor v0.1.1*

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

## Estado Actual (v0.1.1 — completado)

**Lo que funciona hoy:**

| Fase | Módulo | Líneas | Tests | Estado |
|------|--------|--------|-------|--------|
| CLI + Config | `main.rs`, `config.rs` | 620 | 5 | ✅ Done |
| Ingesta RTSP | `ingest.rs` (RetinaReader + reconnect) | 406 | 8 | ✅ Done |
| Decode H.264 | `snapshot.rs` (ffmpeg + PNG saver) | 230 | 5 | ✅ Done |
| Logger JSONL | `logger/` (event, serialize, rotate) | 620 | 8 | ✅ Done |
| Métricas + Health | `metrics.rs` (blind/stale/recovered) | 188 | — | ✅ Done |
| Visualización | `viz.rs` (Rerun bridge) | 119 | — | ✅ Done |
| Pipeline State | `pipeline.rs` (superloop orchestrator) | 64 | — | ✅ Done |
| **Total** | **11 módulos** | **~2,250** | **26** | |

**Lo que está diseñado pero no implementado:**

- `CycleContext` arena (ADR-009)
- `PhaseOutcome` enum (ADR-009)
- `ModelRunner` trait (ADR-009)
- Serde migration (ADR-009 — deferred to v0.2)
- Sistema de errores tipados (`error.rs` — parcialmente)

---

## Sprints y Fases

### 🏗️ Sprint 2 — Inference Core  *(~1 semana)*

> **Objetivo:** Preprocesar frames, correr modelos ONNX, obtener detecciones estructuradas.

```
 FrameBuffer ──▶ Preprocess ──▶ ONNX Session ──▶ Postprocess ──▶ Vec<Detection>
                  (cache)         (per model)       (NMS + scale)
```

**Archivos nuevos:**

| Archivo | Rol | ~Líneas |
|---------|-----|---------|
| `src/preprocess.rs` | Cache de tensores por imgsz, resize+normalize | 120 |
| `src/infer.rs` | Pool de sesiones ORT, dispatch multi-modelo | 180 |
| `src/postprocess.rs` | NMS intra/inter-modelo, escalado, keypoints | 200 |

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
- Pipeline completo corre con `cargo run -- --config config/mana.toml --demo`
- Rerun muestra: cámara + bounding boxes + tracks + zonas + texto de eventos
- JSONL contiene todos los tipos de eventos (frame, detection, track, zone, fsm, health, metrics)
- 26 tests existentes siguen pasando + nuevos tests de integración

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
| **S2 done** | `detect-fast` corre en CPU, genera detecciones | `cargo test` + demo mode con bboxes en Rerun |
| **S3 done** | Tracks persisten entre frames con IDs consistentes | `cargo test` + video de 30s con el mismo track ID |
| **S4 done** | FSM idle→watching→bed_alert→blind con zona real | `cargo test` + `config/fsm.toml` completo evaluado |
| **S5 done** | Pipeline end-to-end con demo mode | `cargo run -- --demo` produce JSONL completo |
| **Release** | 26 tests existentes + nuevos pasan, clippy limpio | CI verde, binario < 20MB |

---

## Deuda Técnica (post-v0.2)

| Item | Prioridad | ADR |
|------|-----------|-----|
| Migrar serializador a serde | Baja | 009 |
| `CycleContext` arena con `clear()` | Media | 009 |
| Reemplazar `AnyReader` enum por trait (static dispatch) | Baja | 009 |
| Sistema de errores tipados completo | Media | — |
| Remover `mana-rtsp` crate (1 fn) → merge a ingest | Baja | — |
| VAAPI/NVDec hardware decode | Baja | — |
| `mana-lite replay --mp4` | Baja | — |
| Zenoh bridge para interop con full Mana OS | Baja | — |
