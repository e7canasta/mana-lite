# ADR-024 — Depth: mapa local al ROI (sin materializar frame completo)

**Estado:** Accepted
**Fecha:** 2026-08-07

**Contexto:** `depth-standard` estima profundidad monocular YOLO26 dentro de
una región de interés fija de la cámara (ROI `[560,140 1240,820]`, 680x680).
La primera integración consideró ejecutar depth sobre frame completo
(1920x1080) para que `DepthMap.data` tuviera la geometría exacta del frame y
no hubiera offsets que resolver. Esa opción fue descartada en la práctica.

**Decisión:** `predict_image()` recibe el ROI y `DepthMap.data` conserva la
geometría **local** del crop. No se crea un buffer full-frame ni se rellena el
exterior con ceros.

1. **Contrato espacial:** `crop_rect`/`roi` es el origen global; los
   consumidores que necesitan una región de la cámara intersectan su región
   global con el ROI y la convierten a coordenadas locales antes de consultar
   el mapa. (Fórmula documentada en `docs/specs/depth-standard.md` §7.)
2. **Métricas:** `valid_pixels` (finitos y > 0), `min_depth_m`, `max_depth_m`
   se calculan sobre el mapa local. `infer_empty` basado en detecciones queda
   separado: `0 dets` no significa depth inválido.
3. **JSONL:** el evento `depth` emite dimensiones y estadísticas, nunca la
   matriz completa. `map_space = "roi"` + `roi` permiten transformar
   coordenadas globales a locales.
4. **Rerun:** BGR completo como contexto + disparity/annotated como crop
   local bajo `/world/camera/crops/<model>/depth/`. El overlay usa alpha
   parcial dentro del ROI y `alpha = 0` fuera — nunca un relleno negro que
   oculte la escena.

**Motivos (memoria y ancho de banda):** expandir a 1920x1080 no agrega
inferencia pero multiplica memoria, colorización y ancho de banda visual por
~4.5 (un mapa f32 full-frame son ~8 MB vs ~1.8 MB del ROI). Si se necesita el
frame completo en Rerun, debe ser una operación exclusiva de visualización,
nunca el contrato interno de las reglas.

**Consecuencias:**
- El probe `depth-image-probe` valida que el mapa permanezca local al ROI
  (shape `[680, 680]` cuando se pasa `--roi 560 140 1240 820`).
- Los tests de postprocess verifican dimensiones locales, offset y valores.
- La evolución futura (`DepthRoiMap` con `roi`, `map_width`, `map_height`,
  `valid_ratio`; consultas por región con mediana/percentiles) debe conservar
  el contrato ROI-local. Ver `docs/specs/depth-standard.md` §15.
