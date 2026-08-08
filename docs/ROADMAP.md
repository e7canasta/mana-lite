# Mana Lite Roadmap

*Última actualización: 2026-08-07 — baseline: depth ROI-local, FP16 benchmark matrix, máscaras seg*

---

## Estado Actual

Pipeline de percepción clínica en un solo binario. El modo operativo es la
**consolidación stateless**: cada frame produce detecciones por modelo y una
observación consolidada, sin identidad temporal.

| Área | Estado | Artefactos |
|---|---|---|
| Ingest RTSP + reconnect | ✅ | ADR-004/007/008 |
| Decode H.264 + snapshots | ✅ | — |
| Inferencia ORT (detect/pose/face/seg/depth) | ✅ | ADR-011/012 |
| Cascade de modelos (`requires`, `same_frame`) | ✅ | ADR-016 |
| Consolidación stateless cross-modelo | ✅ | ADR-017/018 |
| Máscaras + polígonos (seg) | ✅ | ADR-019/020/021/022, `specs/seg-standard.md`, `specs/mask-jsonl.md` |
| Depth ROI-local (root independiente) | ✅ | ADR-024, `specs/depth-standard.md` |
| Pose: keypoints + skeleton en Rerun | ✅ | — |
| Matriz FP16 (4 tasks × 4 tamaños × 2 resoluciones) | ✅ registrada, deshabilitada | `config/models.toml`, `tools/model-tools` |
| Tracking | 🧪 prototipo (predicción lineal + greedy IoU; primera etapa de SORT, ADR-013) | opcional, `track = false` por defecto |
| Zonas + FSM | ✅ motores; validación end-to-end pendiente con tracking | ADR-014/015 |
| JSONL + métricas + Rerun | ✅ | `docs/observability.md` |

### Baseline depth

- `depth-standard` es raíz de cascada, corre sobre ROI fijo `[560,140 1240,820]`
  (mapa local 680x680, `valid_pixels = 462400`).
- No entra en consolidación, tracking, zonas ni FSM.
- Evento JSONL `type=depth` con estadísticas (nunca la matriz).
- Rerun: BGR completo + disparity/annotated local + boxes/polygons de contexto.
- Matriz FP16 completa exportada y registrada deshabilitada para benchmark.

---

## Próximas Etapas

Priorizadas. Cada etapa cambia un solo contrato o habilita un solo rol.

### 1. Benchmark de la matriz FP16 → selección del modelo depth definitivo

- Probar variantes s/m/l/x en 320 y 640 sobre el mismo segmento de video.
- Registrar latencia, rango y calidad; promover una variante (procedimiento en
  `specs/depth-standard.md` §14).
- Definir el presupuesto de latencia de producción antes de elegir.

### 2. `DepthRegionStats` versionado

- Renombrar el contrato interno a `DepthRoiMap` con `roi`, `map_width`,
  `map_height`, `valid_ratio` en el evento JSONL (spec §8/§10).
- Consultas por región: mediana, p10, p90 con intersección global→local (§7).
- Conservar el contrato ROI-local (ADR-024).

### 3. Reglas depth funcionales (`DepthRegionRule`)

- Reglas sin dependencia de modelos: consumen `DepthRegionStats` de una región
  y emiten evidencia numérica.
- No debe hacer que depth gatee face o segmentación (spec §15).
- Calibrar umbrales por cámara y escena antes de integrar con zonas/FSM.

### 4. Tracking: completar SORT y validar

- Kalman 7D + Hungarian del ADR-013 (la interfaz ya está probada con
  predicción lineal + greedy IoU).
- Validar con video real: paciente inmóvil sin drift, oclusión parcial,
  frecuencias distintas entre modelos.
- Recién después: zones/FSM end-to-end y eventos `entity` como salida estándar.

### 5. Reglas clínicas con calibración

- Combinar depth + zonas + FSM en casos de uso clínicos (distancia a borde,
  aproximación/alejamiento) con calibración de escena, sin afirmar distancia
  métrica sin referencia física.

---

## ADRs — Architecture Decision Records

| ADR | Tema | Estado |
|---|---|---|
| [001](adrs/001-single-binary.md) | Single binary architecture | ✅ Accepted |
| [002](adrs/002-toml-catalog-pattern.md) | TOML catalog pattern | ✅ Accepted |
| [003](adrs/003-plc-superloop.md) | PLC superloop execution model | ✅ Accepted |
| [004](adrs/004-retina-rtsp.md) | Retina for RTSP ingest | ✅ Accepted |
| [005](adrs/005-cascaded-inference.md) | Cascaded inference | ✅ Accepted |
| [006](adrs/006-json-lines-stdout.md) | JSON Lines stdout | ✅ Accepted |
| [007](adrs/007-iframe-gating.md) | I-frame gating | ✅ Accepted |
| [008](adrs/008-ingest-engine.md) | Ingest engine design | ✅ Accepted |
| [009](adrs/009-pipeline-design.md) | Pipeline design (arena, coupling, tests) | ✅ Accepted |
| [010](adrs/010-preprocess-cache.md) | Preprocess tensor cache | 🏗️ Draft (sin implementar) |
| [011](adrs/011-inference-engine.md) | Inference engine (ORT pool) | ✅ Accepted |
| [012](adrs/012-postprocess-pipeline.md) | Postprocess (NMS, unified Detection) | ✅ Accepted |
| [013](adrs/013-sort-tracking.md) | SORT tracking for clinical scenes | 🏗️ Draft (primera etapa implementada) |
| [014](adrs/014-zone-engine.md) | Zone engine (spatial + hysteresis) | ✅ Accepted |
| [015](adrs/015-fsm-engine.md) | FSM engine (guards, dwell) | ✅ Accepted |
| [016](adrs/016-cascade-scheduler.md) | Cascade scheduler (interval + requires) | ✅ Accepted |
| [017](adrs/017-detection-consolidation.md) | Detection consolidation across models | ✅ Accepted |
| [018](adrs/018-runtime-stage-boundaries.md) | Runtime stage and publication boundaries | ✅ Accepted |
| [019](adrs/019-import-mana-os-std.md) | Import de std de mana-os (copia de crates) | ✅ Accepted |
| [020](adrs/020-model-enabled-flag.md) | Flag `enabled` por modelo | ✅ Accepted |
| [021](adrs/021-mask-format-compactmask.md) | Formato de máscara (CompactMask + polígonos) | ✅ Accepted |
| [022](adrs/022-mask-overlay-rgba.md) | Overlay de máscaras RGBA en Rerun | ✅ Accepted |
| [023](adrs/023-face-roi-may-exceed-parent.md) | ROI hijo face puede exceder el ROI padre | ✅ Accepted |
| [024](adrs/024-depth-roi-local.md) | Depth: mapa local al ROI, sin full-frame | ✅ Accepted |

## Deuda Técnica

| Item | Prioridad | ADR |
|---|---|---|
| Preprocess tensor cache | Baja | 010 |
| Completar SORT (Kalman + Hungarian) | Media | 013 |
| Migrar serializador a serde | Baja | 009 |
| Sistema de errores tipados completo | Media | — |
| VAAPI/NVDec hardware decode | Baja | — |
| `mana-lite replay --mp4` | Baja | — |
| Zenoh bridge para interop con full Mana OS | Baja | — |
