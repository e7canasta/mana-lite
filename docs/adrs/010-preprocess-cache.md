# ADR-010: Preprocess Tensor Cache

**Status:** Draft
**Date:** 2026-08-04

## Context

Múltiples modelos ONNX pueden pedir el mismo `imgsz`. Por ejemplo `detect-fast` y `face-v12` ambos operan a 320×320. Sin cache, preprocesamos el mismo frame RGB dos veces: 2× resize bilinear + HWC→CHW + normalize `/255.0`. Con 5 modelos y 2-3 imgsz distintos por ciclo, el preproceso puede consumir 6-10ms — comparable al tiempo de inferencia de un modelo liviano.

## Decision

**Caché por imgsz, invalidado cada frame.**

```rust
struct PreprocessCache {
    /// imgsz → tensor normalizado + metadata de escala
    cache: HashMap<u32, PreprocessedFrame>,
}

struct PreprocessedFrame {
    tensor: ort::Tensor<f32>,       // shape [1, 3, H, W] — BCHW
    imgsz: u32,
    ratio: f32,                      // min(imgsz/w, imgsz/h)
    pad_x: u32,                      // padding horizontal (letterbox)
    pad_y: u32,
}

impl PreprocessCache {
    fn get_or_preprocess(&mut self, imgsz: u32, rgb: &[u8], w: u32, h: u32) -> &PreprocessedFrame {
        self.cache.entry(imgsz).or_insert_with(|| preprocess(imgsz, rgb, w, h))
    }

    fn clear(&mut self) {
        self.cache.clear();  // al inicio de cada ciclo con frame nuevo
    }
}
```

**Pipeline de preproceso** (lo que hace `preprocess()`):

```
RGB packed [H×W×3] u8
    │
    ▼ letterbox resize (bilinear, ffmpeg swscale o image crate)
[H'×W'×3] u8  — donde H'=imgsz, W'=imgsz (square), con padding
    │
    ▼ HWC → CHW (transpose)
[3×H×W] f32
    │
    ▼ normalize (/255.0)  ←  opcional, algunos modelos esperan [0,1), otros [0,255]
[3×H×W] f32  →  ort::Tensor
    │
    ▼ guardar ratio, pad_x, pad_y para re-escalar bboxes después
```

**Parámetros de letterbox:**

- `ratio = min(imgsz / w, imgsz / h)` — el factor que preserva aspect ratio
- `new_w = (w * ratio).round()`  — ancho escalado
- `new_h = (h * ratio).round()`  — alto escalado
- `pad_x = (imgsz - new_w) / 2`  — padding izquierdo (y derecho simétrico)
- `pad_y = (imgsz - new_h) / 2`  — padding superior

Estos valores se usan en el postprocesador para convertir bboxes del espacio del tensor al espacio del frame original.

## Why not lazy preprocess on first model use?

Lazy evita preprocesar imgsz que ningún modelo usa en este ciclo. Pero para implementarlo necesitamos saber qué modelos se van a ejecutar *antes* de preprocesar — esto acopla el cache al CascadeScheduler. El eager approach (preprocesar todos los imgsz del catálogo activo) es más simple y el costo es <2ms por imgsz en CPU.

Si en el futuro tenemos 10+ modelos con 5+ imgsz distintos, migraremos a lazy. Por ahora, eager con cache es suficiente.

## Alternatives considered

### A. Preprocesar on-demand sin cache

Cada modelo llama `preprocess()` individualmente. Ventaja: sin estado compartido. Desventaja: 2× costo para imgsz repetidos.

**Rechazado:** El overhead es real. detect-fast + face-v12 comparten imgsz en el 90% de los despliegues. Cachear ahorra 2ms por ciclo.

### B. Preprocesar una sola vez al imgsz más grande, luego crop

Preprocesar a 640×640 (el máximo), los modelos de 320 hacen center-crop. Ventaja: un solo tensor. Desventaja: center-crop no es lo mismo que letterbox — pierde contexto en los bordes.

**Rechazado:** Los modelos se entrenan con letterbox, no con center-crop. Cambiar el preproceso degrada accuracy.

### C. Usar ONNX para el preproceso (añadir nodos al grafo)

Exportar el modelo ONNX con nodos de preproceso incluidos (resize + normalize). Ventaja: el preproceso corre en GPU. Desventaja: modifica el modelo exportado, rompe compatibilidad con modelos de terceros.

**Rechazado:** Los modelos vienen de `ultralytics export` y no incluyen preproceso. No queremos modificar el pipeline de exportación del ML engineer.

## Implementation Notes

- `HashMap::entry().or_insert_with()` da acceso `&PreprocessedFrame` sin mover el ownership. El tensor ORT es clonable (internamente es `Arc`).
- `clear()` se llama al inicio del ciclo de inferencia, justo después de `decode_timed()`.
- El resize puede usar `ffmpeg_next::software::scaling::Context` (ya integrado en `snapshot.rs`) o el crate `image` (también ya en deps). Benchmark necesario: ffmpeg swscale es ~2× más rápido que `image::imageops::resize`, pero `image` no depende de ffmpeg init global.

## Consequences

- **Positive:** 40-60% reducción en tiempo de preproceso para configuraciones multi-modelo típicas.
- **Positive:** Interface simple: `cache.get_or_preprocess(imgsz, rgb, w, h)`.
- **Negative:** Memoria: cada tensor 320×320×3×4 bytes = 1.2MB. Con 3 imgsz distintos = 3.6MB. Aceptable.
- **Negative:** `ort::Tensor<f32>` debe ser clonable/referenciable sin mover ownership. Verificar API de ORT 2.0.0-rc.12.

## References

- ADR-011: Inference Engine
- ADR-012: Postprocess Pipeline
- [Ultralytics letterbox implementation](https://github.com/ultralytics/ultralytics/blob/main/ultralytics/utils/ops.py#L253)
