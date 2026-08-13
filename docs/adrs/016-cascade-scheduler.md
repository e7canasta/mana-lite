# ADR-016: Cascade Scheduler — Interval-Based Model Gating

**Status:** Accepted for scheduler semantics, deployment selection in [ADR-026](026-inference-blueprints.md)
**Date:** 2026-08-04

> Estado de implementación: las dependencias, filtros sobre tracks, scope
> semántico y el gating temporal cooperativo básico están implementados. La
> configuración temporal usa `interval_min_ms` en la regla del blueprint. Las
> urgencias, el catch-up deliberado y la ejecución same-frame dinámica siguen
> pendientes. Ver `docs/subprojects/cooperative-inference-scheduler/`.

## Context

No todos los modelos deben correr cada frame. Un modelo de pose (30ms) no necesita correr a 30fps — con 2fps es suficiente para tracking clínico. Un modelo de profundidad (50ms) puede correr cada 5 segundos. Necesitamos un scheduler que decida qué modelos ejecutar este ciclo basado en:

1. **Intervalo mínimo:** tiempo desde la última ejecución de este modelo.
2. **Modelo padre (requires):** solo ejecutar si el modelo padre encontró algo (ej: pose solo si detect encontró persona).
3. **Scope (bbox crop):** recortar el frame a la región de interés antes de inferir (ej: face solo en la región de la cabeza).

Este scheduler reemplaza el enfoque naive de "ejecutar todos los modelos del estado FSM actual cada ciclo".

## Decision

**Cascade Scheduler con cuatro niveles de gating: schedule → detection → track → semantic scope.**

La cascada no consume detecciones aisladas para activar modelos hijos. El flujo
es: NMS y filtros geométricos, actualización del tracker, confirmación temporal
del track y finalmente evaluación de confianza, área y región semántica. Solo un
track confirmado y visible puede activar un modelo hijo.

```rust
struct CascadeScheduler {
    models: HashMap<String, CascadeEntry>,
}

struct CascadeEntry {
    model_key: String,
    interval_min_ms: u64,              // 0 = cada frame con detección padre
    requires: Option<String>,          // modelo padre (None = root, siempre ejecutar)
    requires_class: Option<String>,    // clase requerida en detecciones del padre
    scope: CascadeScope,              // Full | Crop(track_id)
    last_run_at: Instant,
}

enum CascadeScope {
    Full,                               // frame completo
    Crop { track_id: u64 },            // recortar al bbox del track
    CropClass { class: String },       // recortar al bbox más grande de esta clase
}
```

**API:**

```rust
impl CascadeScheduler {
    /// Decide qué modelos ejecutar este ciclo.
    fn schedule(
        &mut self,
        active_models: &[String],           // modelos requeridos por el estado FSM actual
        detections: &[Detection],           // detecciones del ciclo actual (o anteriores)
        tracks: &HashMap<u64, TrackState>,  // tracks activos
    ) -> Vec<ScheduledRun> {
        let mut runs = Vec::new();

        for model_key in active_models {
            let Some(entry) = self.models.get(model_key) else { continue };

            // 1. ROOT CHECK: si no tiene requires, siempre schedule
            if entry.requires.is_none() {
                if self.interval_elapsed(entry) {
                    runs.push(ScheduledRun { model_key, scope: CascadeScope::Full });
                    continue;
                }
            }

            // 2. PARENT CHECK: ¿el modelo padre produjo detecciones?
            let parent_key = entry.requires.as_ref().unwrap();
            let parent_dets: Vec<&Detection> = detections.iter()
                .filter(|d| d.source_model == *parent_key)
                .collect();

            if parent_dets.is_empty() {
                continue;  // sin detecciones del padre → skip
            }

            // 3. CLASS CHECK: ¿alguna detección del padre es de la clase requerida?
            if let Some(ref required_class) = entry.requires_class {
                let has_class = parent_dets.iter()
                    .any(|d| &d.class == required_class);
                if !has_class {
                    continue;  // sin la clase requerida → skip
                }
            }

            // 4. INTERVAL CHECK: ¿ha pasado suficiente tiempo?
            if !self.interval_elapsed(entry) {
                continue;
            }

            // 5. SCOPE RESOLUTION: ¿recortar o frame completo?
            let scope = match entry.scope {
                CascadeScope::Full => CascadeScope::Full,
                CascadeScope::Crop { .. } => {
                    // Encontrar el track correspondiente
                    // Por ahora: crop al bbox más grande de la clase requerida
                    let best_det = parent_dets.iter()
                        .filter(|d| entry.requires_class.as_ref()
                            .map_or(true, |cls| &d.class == cls))
                        .max_by(|a, b| {
                            let area_a = (a.bbox[2] - a.bbox[0]) * (a.bbox[3] - a.bbox[1]);
                            let area_b = (b.bbox[2] - b.bbox[0]) * (b.bbox[3] - b.bbox[1]);
                            area_a.partial_cmp(&area_b).unwrap()
                        });

                    match best_det {
                        Some(det) => CascadeScope::CropClass {
                            class: det.class.clone(),
                        },
                        None => continue,
                    }
                }
                _ => CascadeScope::Full,
            };

            runs.push(ScheduledRun {
                model_key: model_key.clone(),
                scope,
            });
        }

        // Actualizar last_run_at para los modelos ejecutados
        for run in &runs {
            if let Some(entry) = self.models.get_mut(&run.model_key) {
                entry.last_run_at = Instant::now();
            }
        }

        runs
    }

    fn interval_elapsed(&self, entry: &CascadeEntry) -> bool {
        if entry.interval_min_ms == 0 {
            return true;
        }
        entry.last_run_at.elapsed().as_millis() as u64 >= entry.interval_min_ms
    }
}
```

## Configuracion temporal en el blueprint

La cadencia pertenece a la regla del blueprint, no al catalogo compartido. Esto
permite que cada despliegue ajuste el costo temporal sin duplicar artefactos.

```toml
# config/blueprints/detect-face-pose-seg/blueprint.toml
[[rules]]
model = "detect-fast"

[[rules]]
model = "pose-standard"
requires = "detect-fast"
requires_class = "person"
interval_min_ms = 500           # como maximo 2 Hz

[[rules]]
model = "seg-standard"
requires = "detect-fast"
requires_class = "person"
interval_min_ms = 2000          # como maximo 0.5 Hz
```

## Scope: crop vs full frame

Recortar el frame a una región de interés antes de inferir reduce:
- Resolución de entrada (320×320 en vez de 1920×1080)
- Ruido de fondo (el modelo solo ve la región relevante)
- Tiempo de preproceso (resize más pequeño)

**Implementación del crop:**

```rust
fn crop_roi(rgb: &[u8], frame_w: u32, frame_h: u32, bbox: &[f32; 4]) -> Vec<u8> {
    let (x1, y1, x2, y2) = (bbox[0] as u32, bbox[1] as u32, bbox[2] as u32, bbox[3] as u32);
    let crop_w = (x2 - x1).min(frame_w - x1);
    let crop_h = (y2 - y1).min(frame_h - y1);
    let mut cropped = Vec::with_capacity((crop_w * crop_h * 3) as usize);
    for y in y1..y1+crop_h {
        let row_start = (y * frame_w + x1) as usize * 3;
        cropped.extend_from_slice(&rgb[row_start..row_start + crop_w as usize * 3]);
    }
    cropped
}
```

## Interacción con FSM state

El FSM state declara qué modelos necesita:

```toml
[fsm.states.watching]
models = ["detect-fast", "pose-standard"]
```

Pero el cascade scheduler **no ejecuta** `pose-standard` si las condiciones no se cumplen (sin persona detectada, intervalo no cumplido). Esto es distinto de "ejecutar si o sí". El cascade es un filtro sobre los modelos declarados por el FSM, no un reemplazo.

## Ejemplo de scheduling en un ciclo

```
FSM state = "watching" → active_models = ["detect-fast", "pose-standard", "face-v12"]

CascadeScheduler::schedule():
  detect-fast:    root, interval=0       → RUN (full frame)
  pose-standard:  requires=detect-fast, interval=500ms, requiere_class=person
                  → detect-fast encontró "person"? SÍ
                  → última ejecución hace 600ms? SÍ → RUN (crop a person bbox)
  face-v12:       requires=pose-standard, interval=1000ms
                  → pose-standard se ejecutó este ciclo?
                  → NO (se ejecutará AHORA, aún no hay detecciones de pose)
                  → SKIP (se ejecutará el próximo ciclo si pose encontró keypoints)

Resultado: 2 modelos ejecutados de 3 posibles → 33% ahorro GPU
```

## Why not FSM-driven only (ADR-005)?

ADR-005 proponía que el FSM state es el único driver de qué modelos ejecutar. Esto es suficiente para v0.4 pero insuficiente para optimización:

- `pose-standard` en state "watching" corre aunque no haya persona → desperdicio
- `face-v12` corre aunque la persona esté de espaldas → desperdicio
- Sin interval scheduling, modelos caros corren en cada I-frame → sobrecarga

El cascade scheduler es la implementación de v0.5 según ADR-005.

## Consequences

- **Positive:** 30-70% reducción en tiempo de inferencia por ciclo para configuraciones multi-modelo.
- **Positive:** Scope crop reduce resolución de entrada → modelos más rápidos y precisos (menos fondo).
- **Positive:** Configuración declarativa por modelo → el ML engineer decide la política, no el código.
- **Negative:** Una falla en cascade (detect-fast no detecta persona) bloquea en cadena todos los modelos dependientes. Mitigado: root models (sin requires) siempre corren → siempre hay detección base.
- **Negative:** Scope crop introduce complejidad de coordenadas: el postprocesador debe recibir el offset del crop para mapear bboxes de vuelta al espacio del frame original. El PreprocessedFrame ya tiene `ratio` y `pad` — el crop añade `offset_x`, `offset_y`.

## References

- ADR-005: Cascaded Inference (estrategia original)
- ADR-010: Preprocess Cache
- ADR-011: Inference Engine
- ADR-012: Postprocess Pipeline
- `config/models.toml` — cascade fields
