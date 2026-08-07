# ADR-022 — Overlay de máscaras en Rerun: RGBA (no SegmentationImage u16)

**Estado:** Accepted
**Fecha:** 2026-08-06
**Contexto:** visualizar las máscaras de `seg-standard` en Rerun. La
arquitectura de rerun soporta `SegmentationImage` (u16 class-id + colormap),
pero la std de mana-os ya resuelve esto con un overlay pintado en RGBA
(`log_segmentation_overlay` en `mana-rerun-common`).

**Decisión:** adoptar el patrón de mana-os:
1. Pintar un buffer overlay u8 (0 = fondo, 1..8 = id de instancia) en el
   espacio de máscara, rasterizando cada `CompactMask` en su offset.
2. Colorear con paleta de 8 colores RGBA (alpha 120) — idéntica a la de
   mana-os.
3. Loguear como `rerun::Image::from_rgba32` en `/world/camera/masks/{model}`.

**Revisión (2026-08-06):** los polígonos de contorno se logueaban como
`LineStrips2D`, pero en el viewer se ven como líneas sueltas sin contexto.
Se reemplazó por imágenes de debug dibujadas por mana-lite (toggle
`mask_debug`, default false), en `/world/camera/debug/{model}/mask` y
`/world/camera/debug/{model}/polygon`: el polígono se rasteriza con
`imageproc::drawing::draw_line_segment_mut` mapeando los vértices
(frame-normalizados) de vuelta al espacio de máscara
(`p_mask = (p_frame * frame − origin) / mask_dims`), de modo que ambas
imágenes comparten resolución y origen y el contorno es verificable contra
la máscara.

**Racional:**
- Reutiliza el código de la std (patrón probado, colores consistentes con el
  ecosistema mana-os).
- RGBA es directamente overlayeable sobre `/world/camera/crops/{model}/bgr`
  en el viewer, sin dependencia de colormaps.
- Evita mantener dos formatos de transporte (el u16 de `SegmentationImage`
  no se corresponde con el wire del JSONL).
- Las imágenes de debug usan `decode_crop` (crop del RLE), no `to_dense`
  (imagen completa) — un `to_dense` indexado como crop pinta vacío en
  silencio (bug detectado por test de render).
- El overlay RGBA (toggle `masks`) también lleva el contorno del polígono
  pintado encima del relleno (sentinel `0xFF` → blanco, alpha 255), de modo
  que el borde se vea siempre, sin depender de `mask_debug`.
- Los contornos simplificados se loguean además como primitiva 2D real
  (`LineStrips2D` cerrado, coordenadas de frame, color por instancia) en
  `/world/camera/mask_polygons/{model}/{i}` (toggle `mask_polygons`,
  default true). El número de vértices lo controla `polygon_simplify`
  (epsilon de simplificación, default 0.75, configurable por modelo en
  `models.toml`): mayor epsilon = polígono más simple en el viewer.

**Consecuencias:** toggles `masks` (default true) y `mask_debug` (default
false) en `viz_data.send`; el overlay solo se emite cuando la inferencia
devuelve `Results.masks`.
