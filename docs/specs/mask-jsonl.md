# Spec-003 — Wire JSONL de máscaras de instancia

## Evento

Los registros `detection` (por modelo) pueden incluir por cada detección un
campo `mask` self-contained. La máscara vive en *espacio de máscara* (la
imagen pasada al modelo: el crop para ramas de cascada).

## Schema

```json
{
  "type": "detection",
  "frame_id": 123,
  "model": "seg-standard",
  "det": [
    {
      "class": "person",
      "confidence": 0.87,
      "bbox": [560.0, 140.0, 1240.0, 820.0],
      "mask": {
        "rle": [0, 42, 3, 55, ...],
        "bbox": [12, 30, 180, 260],
        "origin": [560, 140],
        "mask_dims": [680, 680],
        "polygons": [[[0.57, 0.21], [0.61, 0.20], ...], ...]
      }
    }
  ]
}
```

| Campo | Tipo | Significado |
|---|---|---|
| `rle` | u32[] | Runs RLE column-major de la máscara recortada al bbox (codec `vernier-mask`) |
| `bbox` | f32[4] | Bbox de la máscara dentro del espacio de máscara; `bbox[2]-bbox[0]` = ancho RLE, `bbox[3]-bbox[1]` = alto RLE |
| `origin` | u32[2] | Posición `[x, y]` del espacio de máscara dentro del frame original |
| `mask_dims` | u32[2] | Tamaño `[w, h]` del espacio de máscara (= dims del crop) |
| `polygons` | f32[][2][] | Contornos simplificados (RDP, simplify 0.75, threshold 0.5), normalizados al frame |

## Decodificación lossless (consumidor)

1. `h = bbox[3]-bbox[1]`, `w = bbox[2]-bbox[0]`.
2. `Rle::from_counts(h, w, rle)` (vernier-mask 0.2) → `to_raster_bytes()` →
   raster binario del recorte (1 = foreground).
3. Raster en frame: pixel `(bbox[1]+y+origin[1], bbox[0]+x+origin[0])`.

Round-trip verificado por tests (`mask_wire_record_round_trips_through_rle`,
`detection_emits_mask_wire_record`).

## Reglas

- El campo `mask` se omite cuando la detección no tiene máscara (modelos
  `detect` sin salida de segmentación).
- `polygons` siempre frame-normalizados; `rle`+`bbox`+`origin`+`mask_dims`
  son la fuente de verdad lossless.
