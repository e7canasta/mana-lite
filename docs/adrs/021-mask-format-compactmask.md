# ADR-021 — Formato de máscara: CompactMask + polígonos (fuente de verdad)

**Estado:** Accepted
**Fecha:** 2026-08-06
**Contexto:** la rama `seg-standard` produce máscaras de instancia. Se
necesita un formato lossless de transporte (JSONL) y representaciones
derivadas (polígonos) sin duplicar lógica — el código ya existe en mana-os.

**Decisión:**
1. **Fuente de verdad:** `CompactMask` (crop-RLE column-major acotado al
   bbox, codec `vernier-mask 0.2`) tal como mana-os lo usa en su bus. La
   máscara se almacena en *espacio de máscara* (la imagen pasada al modelo,
   i.e. el crop para modelos en cascada), con `origin` (posición del espacio
   de máscara en el frame) y `mask_dims` explícitos.
2. **Representación derivada:** polígonos simplificados vía
    `mask_to_polygons` (Suzuki-Abe + RDP), con `polygon_simplify` y
    `mask_threshold` configurables por modelo en `models.toml`, normalizados al
    frame, para consumo liviano.
3. **Pipeline (mana-lite):** el crate `ultralytics-inference` ya entrega
   `Results.masks` (N,H,W) post-sigmoid alineados por índice con los bboxes,
   en el espacio de la imagen pasada al modelo. En `InferEngine::run`
   (src/infer.rs) se binariza con threshold, se recorta al bbox y se
   construye el `CompactMask` + polígonos. El offset al frame se aplica ahí
   mismo (`origin`), y los polígonos se normalizan al frame.
4. **Consumidores:** `Detection`/`DetectionEvidence` portan la máscara; el
   JSONL (Spec-003) la emite en forma self-contained; el overlay de Rerun
   (ADR-022) rasteriza desde `CompactMask`/polígonos.

**Alternativas descartadas:**
- RLE propio o downsample a 128px: duplicaba lógica ya probada.
- `SegmentationImage` u16 de Rerun para overlay: ver ADR-022.

**Consecuencias:** formato idéntico al del ecosistema mana-os (los
consumidores de mana-os pueden reutilizar `CompactMaskHeader::parse`),
pruebas unitarias de round-trip cubren decode desde RLE (tests
`compact_mask` heredados).
