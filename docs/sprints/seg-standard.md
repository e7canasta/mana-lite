# Sprint: Rama `seg-standard` (segmentación + máscaras + polígonos)

**Duración estimada:** 4 semanas (fases semanales)
**Objetivo:** agregar la tercera rama hermana de `face`/`pose` bajo `detect-fast`
con `yolo26n-seg.onnx`, gating `same_frame`, bboxes + máscaras CompactMask +
polígonos de contorno en v1, y flag `enabled` por modelo.
**Origen del código:** importar std propias de `/home/care/mana-1001/mana-os`
(copiar módulos/crates, nunca reescribir ni mover — el origen queda intacto).

## Decisiones (confirmadas)

| Decisión | Valor |
|---|---|
| La rama seg existe (3 hermanos sin dependencias entre sí) | sí |
| Modelo | `models/yolo26n-seg.onnx` |
| Gating | `same_frame` (funciona con `track=false`) |
| Máscaras en v1 | sí (bboxes + máscaras + polígonos) |
| Toggle de rama | flag `enabled` por modelo en models.toml (NO `disabled_tasks`) |
| Reglas futuras (cuadrante de ROI `[560,140,1240,820]`, bordes) | fuera de scope |
| Import | copiar `compact_mask.rs`, `polygon.rs`, `primitives.rs`, `transform.rs` de mana-geometry + `mask_to_polygons`/`compute_mask_obb` de mana-annotate |

## Fase 0 — Semana 1: Fundación (config + import de std)

**ADR-019** (importación de std desde mana-os) y **ADR-020** (flag `enabled`).
**Spec-001**: schema `[models.*]` con `enabled` (default `true`).

Tasks:
- [x] Flag `enabled: bool` en `ModelConfig` (default `true`, sin romper configs actuales).
- [x] Aplicar `enabled` en el cascade scheduler: rama deshabilitada no programa inferencia.
- [x] Nueva crate workspace `std/mana-geometry` con módulos copiados limpios
      (sin dep de mana-types): `compact_mask.rs`, `polygon.rs`, `primitives.rs`, `transform.rs`.
- [x] Deps: `vernier-mask = "0.2"`, `thiserror`, `imageproc = "0.25"`, `image = "0.25"`.
- [x] Copiar `mask_to_polygons` + `compute_mask_obb` desde mana-annotate como módulo
      `polygonize` en mana-geometry.
- [x] Copiar tests asociados desde mana-os (compact_mask, polygon) y adaptar.
- [x] `cargo build` + `cargo test` en verde (workspace completo).
- [x] ADR-019, ADR-020 y Spec-001 escritos en `docs/adrs/` y `docs/specs/`.

**Done:** workspace compila, tests de CompactMask/polygon pasan, ramas apagables.

## Fase 1 — Semana 2: Inferencia + máscaras

**ADR-021** (formato de máscara: CompactMask como fuente de verdad + polígonos derivados).
**Spec-002**: pipeline de la rama `seg-standard` (crop `largest_class` margin 0.15,
`requires_class = "person"`, `same_frame = true`; las políticas de máscara
`mask_threshold` y `polygon_simplify` viven en `models.toml`.

Tasks:
- [x] `[models.seg-standard]` en `config/models.toml` (`yolo26n-seg.onnx`).
- [x] Regla cascade `seg-standard` en `config/cascade.toml`.
- [x] Productor de máscaras en `src/infer.rs`: capturar `Results.masks` (espacio
      crop, ya post-sigmoid, alineadas por índice con bboxes) → threshold →
      bbox crop → `CompactMask` + offset a frame + polígonos normalizados a frame.
- [x] Consolidación: adjuntar máscara/polígonos como evidencia en
      `DetectionEvidence` (person sigue siendo la entidad primaria).
- [x] Test: una persona sintética produce bbox + CompactMask + polígono coherentes.

**Done:** `seg-standard` corre con `track=false` y enriquece la observación.

## Fase 2 — Semana 3: Salidas (JSONL + Rerun)

**ADR-022** (overlay RGBA vía `rerun::Image::from_rgba32` en vez de
`SegmentationImage` u16, patrón heredado de mana-rerun-common).
**Spec-003**: schema JSONL de máscara: `polygons` (frame-normalizados, simplificados),
`rle_counts` (CompactMask), `offset` + `crop` (fidelidad lossless para consumidores).

Tasks:
- [x] Extender `src/logger/event.rs` + `serialize.rs` con el payload de máscara.
- [x] Round-trip test: JSONL → decode → CompactMask/polígonos.
- [x] Overlay RGBA en `src/viz.rs` (pintado class-id + paleta 8 colores alpha 120,
      patrón de `mana-rerun-common/src/logging/segmentation.rs`).
- [x] ADR-022 y Spec-003 escritos.

**Done:** salida JSONL con máscaras y overlay visible en Rerun.

## Fase 3 — Semana 4: E2E + documentación

Tasks:
- [x] Verificación end-to-end con video de muestra: JSONL + Rerun.
- [x] Ejemplo de config en `config/` con `enabled = false` para face y seg.
- [x] Actualizar `docs/ARCHITECTURE.md` (sección cascade) y `docs/ROADMAP.md`.
- [x] `cargo clippy`: **cero warnings nuevos** (verificado por diff de lints contra
      el baseline: el repo trae 299 warnings pre-existentes — metrics.rs, pipeline.rs,
      zones.rs, la mitad de viz.rs, config.rs, serialize.rs — no tocados; el código
      nuevo lleva `#[allow]` puntuales donde el lint es exigido por el patrón heredado).
- [x] Nota de rendimiento (E2E real, 2026-08-06, cámara RTSP `mana015`, build debug):
      por llamada CPU — `detect-fast` ~115 ms, `seg-standard` ~76 ms (crop a la
      persona más grande, roi ~282×668), `face-yolo` ~476 ms (yolov12l). El ciclo
      queda en ~0.3 Hz limitado por face-yolo + la secuencia detect→children.

**Done:** feature usable y documentada; deuda de clippy = baseline pre-existente (0 nuevos).

## Referencias (fuente de import, mana-os)

- `/home/care/mana-1001/mana-os/crates/std/mana-geometry/src/compact_mask.rs` (1225 ln)
- `/home/care/mana-1001/mana-os/crates/std/mana-geometry/src/polygon.rs` (177 ln)
- `/home/care/mana-1001/mana-os/crates/std/mana-geometry/src/primitives.rs` (1621 ln)
- `/home/care/mana-1001/mana-os/crates/std/mana-geometry/src/transform.rs` (171 ln)
- `/home/care/mana-1001/mana-os/crates/std/mana-annotate/src/geom/polygon.rs` (291 ln: `mask_to_polygons`)
- `/home/care/mana-1001/mana-os/crates/std/mana-annotate/src/geom/obb.rs` (214 ln: `compute_mask_obb`)
- `/home/care/mana-1001/mana-os/crates/tools/mana-rerun-common/src/logging/segmentation.rs` (overlay RGBA)
- `/home/care/mana-1001/mana-os/crates/inference/mana-yolo/src/segment.rs` (pipeline de referencia, defaults)
- Tests: `mana-yolo/tests/compact_mask_e2e.rs`, `mana-annotate/examples/mask_decoding.rs`
