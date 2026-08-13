# Spec-005 — Rama `pose-standard` (pipeline)

## Modelo

```toml
[models.pose-standard]
profile = "person"
enabled = true
path = "tools/model-tools/artifacts/yolo26-fp16/yolo26s-pose-fp16-320.onnx"
task = "pose"
confidence = 0.40
iou = 0.50
half = true
imgsz = 320

[models.pose-standard.crop]
type = "largest_class"
class = "person"
margin = 0.15

[models.pose-standard.postprocess]
allow_classes = ["person"]
min_confidence = 0.10
min_area_ratio = 0.01
max_area_ratio = 1.0
nms_iou = 0.50
```

## Regla de cascada (blueprint)

```toml
[[rules]]
model = "pose-standard"
requires = "detect-fast"
requires_class = "person"
requires_exact_count = 1
requires_min_confidence = 0.50
same_frame = false
```

- Ejecuta con gate sobre el **track confirmado** (`same_frame = false`, igual
  que la rama de face): un dropout corto del detector no apaga al hijo.
- El crop se resuelve sobre el bbox de ese track con el `largest_class` de
  persona y margen 0.15.
- Los keypoints emitidos por el modelo se re-mapean del espacio del crop al
  frame antes de consolidar.

## Salidas

- `Detection.keypoints: Option<Vec<[f32; 3]>>` por detección de
  `pose-standard`, re-mapeados al frame (`translate_keypoints`).
- Rerun: esqueleto por persona (`ModelRole::Skeleton`).
- JSONL: la detección de pose se registra como `detection` con bbox,
  confianza y campo `keypoints` (`[[x, y, conf], ...]` en coordenadas de
  frame). Los modelos sin keypoints omiten el campo.

## Criterios de aceptación

- El ONNX carga y su task declarada coincide con la del catálogo.
- La compuerta gobierna: con exactamente una persona confirmada y visible el
  hijo corre; sin ella se saltea y se cuenta (`skips`).
- El recorte sigue al track (el `roi` reportado se mueve con la persona).
- La rama no aparece ni en detecciones ni en `skips` cuando el modelo no está
  en el blueprint (`enabled` del catálogo queda sobrescrito por la selección).
