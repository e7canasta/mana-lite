# Spec-002 — Rama `seg-standard` (pipeline)

## Modelo

```toml
[models.seg-standard]
path = "models/yolo26n-seg.onnx"
task = "detect"          # segment → detect en el catálogo actual
confidence = 0.30
iou = 0.5
imgsz = 640

[models.seg-standard.crop]
type = "largest_class"
class = "person"
margin = 0.15

[models.seg-standard.postprocess]
allow_classes = ["person"]
min_confidence = 0.25
min_area_ratio = 0.01
max_area_ratio = 1.0
min_component_area_ratio = 0.01
nms_iou = 0.50
```

## Regla de cascada (config/cascade.toml)

```toml
[[rules]]
model = "seg-standard"
requires = "detect-fast"
requires_class = "person"
same_frame = true
```

- Ejecuta en el mismo frame que `detect-fast` (gating `same_frame`, igual que
  face), sin depender del tracking.
- El crop es el bbox `largest_class` de persona con margen 0.15.
- La máscara emitida vive en el espacio del crop (`mask_dims`), con `origin`
  apuntando al frame.
- Los componentes desconectados de la máscara menores que
  `min_component_area_ratio` se eliminan antes de construir `CompactMask` y
  polígonos. Los componentes grandes se conservan dentro de la misma persona.

## Políticas de máscara

| Política | Valor | Uso |
|---|---|---|
| `mask_threshold` | 0.5 | Binarización de la máscara f32 |
| `polygon_simplify` | 0.90 | Simplificación RDP (porcentaje) para `seg-standard` |

## Salidas

- `Detection.mask: Option<DetectionMask>` → `DetectionEvidence.mask` tras
  consolidación (la persona de `seg-standard` se fusiona con la de
  `detect-fast` por IoU; la máscara viaja como evidencia).
- JSONL: Spec-003 (campo `mask` por detección).
- Rerun: overlay RGBA (ADR-022).

## Criterios de aceptación

- Corre con `track = false` en `config/mana.toml`.
- Una persona sintética produce bbox + `CompactMask` + polígono coherentes
  (tests en `src/infer.rs`).
- `enabled = false` en `[models.seg-standard]` elimina la rama sin efectos en
  las demás.
