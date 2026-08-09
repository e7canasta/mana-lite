# ADR-019 — Importación de std desde mana-os (segmentación)

**Estado:** Accepted
**Fecha:** 2026-08-06
**Contexto:** para la rama `seg-standard` se necesita formato de máscara,
poligonización y geometría. Existe implementación madura y probada en el
workspace hermano `/home/care/mana-1001/mana-os` (crates std propios del
proyecto). Reescribir sería duplicar lógica y riesgo.

**Decisión:** importar por copia (vender) los módulos std necesarios de
mana-os a la nueva crate workspace `std/mana-geometry` de mana-lite:

- `compact_mask.rs`, `polygon.rs`, `transform.rs`,
  `bbox.rs`, `iou.rs` — de `mana-os/crates/std/mana-geometry`.
- `mask_to_polygons` + `compute_mask_obb`/`compute_polygon_obb` — de
  `mana-os/crates/std/mana-annotate/src/geom/` (módulo `polygonize`).

**Adaptaciones (registradas):**
- Se eliminan dependencias de `mana_types` (tipos ABI del bus de mana-os):
  - `bbox.rs`: `box_to_pixels`, `clamp_box`/`area` reescritos sin
    `DetectionV1` (tuplas puras).
  - `iou.rs`: `compute` (DetectionV1) eliminado; se conservan
    `box_overlap`, `box_overlap_batch`, `with_zone`.
- `zones.rs` y `nms.rs` no se importan (acoplados a `SceneEntityV1`/`ZoneV1`
  de mana-os; mana-lite no tiene zone-bus).
- Dependencias: `vernier-mask 0.2` (codec RLE), `image 0.25`,
  `imageproc 0.25` (Suzuki-Abe + RDP), `thiserror`, `proptest` (dev).
- Los módulos copiados son **fuente de verdad única** en mana-lite; mana-os
  sigue intacto (solo lectura). Cambios futuros se portan manualmente.

**Consecuencias:** la crate es agnóstica de ABI, testeada (151+3 tests
heredados en verde), y reutilizable por consumidores futuros (reglas de
cuadrante, zona-cerebro).
