# ADR-012: Postprocess Pipeline & Unified Detection Type

**Status:** Draft
**Date:** 2026-08-04

## Context

Cada modelo ONNX produce tensores crudos en formatos específicos por task. Detect produce `[1, 84, 6300]` (4 bbox + 80 clases para 6300 anchors). Pose produce `[1, 56, 6300]` (4 bbox + 1 conf + 17 keypoints × 3). Segment produce bboxes + mask coefficients + protos.

Necesitamos convertir estos formatos heterogéneos en un tipo de detección unificado que el tracker, las zonas y el logger puedan consumir sin conocer el modelo de origen.

## Decision

**Tipo unificado `Detection` + trait `Postprocessor` por task.**

```rust
/// Detección unificada — el tipo que fluye a través de INFER → TRACK → ZONES → FSM.
struct Detection {
    class: String,               // "person", "face", "chair"...
    confidence: f32,             // 0.0–1.0
    bbox: [f32; 4],             // x1, y1, x2, y2 — normalized [0,1] or pixel coords
    keypoints: Vec<[f32; 3]>,  // (x, y, conf) — solo pose; vacío para detect
    mask: Option<Vec<u8>>,      // máscara binaria comprimida — solo segment
    source_model: String,        // "detect-fast", "pose-standard"...
    source_task: String,         // "detect", "pose", "segment"...
}
```

**Trait de postprocesador:**

```rust
trait Postprocessor {
    /// Convierte tensores ORT crudos → Vec<Detection> en coordenadas del frame original.
    fn process(
        &self,
        outputs: &[ort::Value],          // tensores crudos de session.run()
        config: &ModelEntry,              // confidence, iou, max_det thresholds
        orig_w: u32, orig_h: u32,        // dimensiones originales del frame
        ratio: f32, pad_x: u32, pad_y: u32,  // metadata del preprocess
    ) -> Vec<Detection>;
}

struct DetectPostprocessor;
struct PosePostprocessor;
struct SegmentPostprocessor;
struct ClassifyPostprocessor;
struct DepthPostprocessor;    // devuelve 1 Detection con mask = depth map
```

## Algoritmo de postproceso (Detect)

```
1. Extraer bbox [x_center, y_center, w, h] del tensor YOLO [1, 4+classes, N_anchors]
2. Aplicar sigmoid a x_center, y_center + offset de grid
3. Aplicar exp a w, h + anchor scaling
4. Convertir xywh → x1y1x2y2
5. Aplicar confianza threshold (del ModelEntry.confidence)
6. NMS: ordenar por confianza, eliminar solapados con IoU > ModelEntry.iou
7. Re-escalar coordenadas del espacio letterbox → espacio original:
   x_orig = (x_letterbox - pad_x) / ratio
   y_orig = (y_letterbox - pad_y) / ratio
8. Clamp a [0, orig_w] × [0, orig_h]
9. Aplicar los filtros configurados para el modelo: clase permitida, confianza,
   área mínima/máxima y bbox válido.

Los filtros se aplican después de restaurar coordenadas y antes de publicar la
salida del modelo. Por tanto, la misma `Vec<Detection>` filtrada es la única
que consumen tracking, métricas, Rerun y JSONL. Los filtros de cascada
(`requires_*`) son posteriores y solo deciden si un track válido habilita un
modelo hijo.

Mana Lite también ejecuta un NMS defensivo explícito sobre la salida
normalizada, por clase y usando `ModelEntry.iou`. El contador
`post_nms_suppressed` permite distinguir las cajas que el propio proceso
suprimió, incluso cuando el modelo exportado ya incluye NMS. La deduplicación
entre modelos no se resuelve con NMS: pertenece a `DetectionConsolidator`.
```

## NMS: intra-modelo vs inter-modelo

**Intra-modelo (siempre):** Cada postprocesador aplica NMS a sus propias detecciones. Detect-fast con detect-fast. Esto es estándar YOLO.

**Inter-modelo:** La deduplicación entre salidas se realiza en la capa de
consolidación de detecciones. `detect-fast` y `pose-standard` pueden conservar
ambas evidencias, pero producen una sola `ConsolidatedObservation` y un solo
bbox de escena.

```rust
fn inter_model_nms(detections: &mut Vec<Detection>, iou_threshold: f32) {
    detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    let mut keep = vec![true; detections.len()];
    for i in 0..detections.len() {
        if !keep[i] { continue; }
        for j in i+1..detections.len() {
            if !keep[j] { continue; }
            if iou(&detections[i].bbox, &detections[j].bbox) > iou_threshold {
                keep[j] = false;
            }
        }
    }
    // preservar el de mayor confianza, eliminar el otro
    let mut idx = 0;
    detections.retain(|_| { let k = keep[idx]; idx += 1; k });
}
```

**Decisión:** NMS intra-modelo con el umbral `ModelEntry.iou`; consolidación
espacial posterior para relaciones entre modelos.

## Pose postprocessor

Igual que Detect, pero además extrae keypoints:

```
Del tensor YOLO pose [1, 4+1+51, N_anchors]:
- Primeros 5 canales: bbox (4) + objectness (1) → igual que detect
- Canales 5+: 17 keypoints × (x, y, confidence)

Para cada keypoint:
  x_kp = (sigmoid(x) * 2 - 0.5 + grid_x) * stride
  y_kp = (sigmoid(y) * 2 - 0.5 + grid_y) * stride
  conf_kp = sigmoid(conf)
```

## Classify postprocessor

```
Del tensor YOLO classify [1, N_classes]:
- softmax sobre N_classes
- top-k (k configurable, default 5)
- devuelve Vec<Detection> con bbox = frame completo ([0,0,w,h])
```

## Depth postprocessor

```
Del tensor depth [1, H, W]:
- normaliza a [0, 255] u8
- devuelve 1 Detection con mask = depth_map comprimido (PNG o raw)
- class = "depth", confidence = 1.0
```

## Unified Detection vs mana_types::DetectionBatchV1

El crate `mana-types` define `DetectionBatchV1` con bbox, class_id, confidence, track_id, keypoints. ¿Reutilizarlo o crear uno nuevo?

**Decisión:** Crear `Detection` local en `postprocess.rs`. El tipo upstream es más complejo (incluye track_id, timestamps, ROI metadata) y está acoplado al formato de serialización de la capa iceoryx2. Nuestro tipo es más simple y específico al pipeline de Mana Lite. En v0.3, podemos añadir un `From<Detection> for DetectionBatchV1` para interoperabilidad con herramientas de visualización (mana-viz).

## Consequences

- **Positive:** Un solo tipo fluye por todo el pipeline. Tracker, zonas y FSM dependen solo de `Detection`, no de formatos ORT.
- **Positive:** NMS inter-modelo elimina duplicados cross-model (misma persona detectada por dos modelos).
- **Negative:** `keypoints: Vec<[f32; 3]>` y `mask: Option<Vec<u8>>` son campos opcionales que el 80% de las detecciones no usan. Costo de memoria: ~60 bytes extra por detección. Con 50 detecciones = 3KB. Irrelevante.
- **Negative:** Postprocesadores específicos por task requieren mantenimiento cuando ORT cambia el formato de salida. Mitigado por tests con modelos YOLO exportados.

## References

- ADR-010: Preprocess Cache (letterbox metadata)
- ADR-011: Inference Engine
- ADR-013: SORT Tracking (consume Vec<Detection>)
- [YOLO output format reference](https://docs.ultralytics.com/modes/predict/#working-with-results)
