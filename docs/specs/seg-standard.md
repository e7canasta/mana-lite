# Spec-002 — Rama `seg-standard` (pipeline)

## Modelo

```toml
[models.seg-standard]
profile = "person-seg"
enabled = true
path = "tools/model-tools/artifacts/yolo26-fp16/yolo26s-seg-fp16-320.onnx"
task = "segment"
confidence = 0.20
iou = 0.10
half = true
imgsz = 320
polygon_simplify = 0.98

[models.seg-standard.crop]
type = "largest_class"
class = "person"
margin = 0.15

[models.seg-standard.postprocess]
allow_classes = ["person"]
min_confidence = 0.25
min_area_ratio = 0.05
max_area_ratio = 1.0
min_component_area_ratio = 0.05
mask_threshold = 0.5
nms_iou = 0.10
```

## Regla de cascada (blueprint)

```toml
[[rules]]
model = "seg-standard"
requires = "detect-fast"
requires_class = "person"
requires_exact_count = 1
requires_min_confidence = 0.50
same_frame = false
```

- Ejecuta con gate sobre el **track confirmado** (`same_frame = false`, igual
  que la rama de face): un dropout corto del detector no apaga al hijo, y el
  blueprint declara `requires_tracking = true`.
- El crop es el bbox del track de persona con margen 0.15.
- La máscara emitida vive en el espacio del crop (`mask_dims`), con `origin`
  apuntando al frame.
- Los componentes desconectados de la máscara menores que
  `min_component_area_ratio` se eliminan antes de construir `CompactMask` y
  polígonos. Los componentes grandes se conservan dentro de la misma persona.

## Políticas de máscara

| Política | Valor | Uso |
|---|---|---|
| `mask_threshold` | 0.5 | Binarización de la máscara f32 |
| `polygon_simplify` | 0.98 | Simplificación RDP (porcentaje) para `seg-standard` |

## Salidas

- `Detection.mask: Option<DetectionMask>` → `DetectionEvidence.mask` tras
  consolidación (la persona de `seg-standard` se fusiona con la de
  `detect-fast` por IoU; la máscara viaja como evidencia).
- JSONL: Spec-003 (campo `mask` por detección).
- Rerun: overlay RGBA (ADR-022).

## Criterios de aceptación

- Requiere `track = true` en `config/mana.toml` (el blueprint declara
  `requires_tracking = true`).
- Una persona sintética produce bbox + `CompactMask` + polígono coherentes
  (tests en `src/infer.rs`).
- La rama no aparece ni en detecciones ni en `skips` cuando el modelo no está
  en el blueprint (`enabled` del catálogo queda sobrescrito por la selección).
