# ADR-013: SORT Tracking for Clinical Scenes

**Status:** Draft
**Date:** 2026-08-04

## Context

Las detecciones por frame son efímeras — no tienen identidad. Necesitamos tracking para:

1. **Identidad persistente:** saber que la persona en frame N es la misma que en frame N+1 (mismo `track_id`).
2. **Conteo de ocupantes:** ¿cuántas personas hay en la habitación? Sin tracking, no sabemos si dos detecciones en frames consecutivos son la misma persona.
3. **Eventos clínicos con sujeto:** "el paciente (track 1) salió de la cama", no "alguien salió de la cama".
4. **Filtrado de falsos positivos:** detecciones esporádicas (1 frame, confianza baja) → ignorar. Detecciones persistentes (5+ frames) → confirmar.

## Decision

**SORT clásico (Simple Online and Realtime Tracking) con Kalman 7D + Hungarian matching.**

SORT fue publicado en 2016 (Bewley et al.) y es el algoritmo estándar en MOTChallenge. 7 dimensiones de estado: `[x, y, s, r, dx, dy, ds]` donde `s = w*h` (escala), `r = w/h` (aspect ratio).

```rust
struct TrackingEngine {
    tracks: HashMap<u64, TrackState>,
    next_id: u64,
    max_age: u32,            // 10 frames sin detección → eliminar track
    min_hits: u32,           // 3 detecciones consecutivas → track confirmado
    iou_threshold: f32,      // 0.3 — umbral de matching
}

struct TrackState {
    id: u64,
    class: String,
    bbox: [f32; 4],          // x1, y1, x2, y2 — posición actual
    kalman: KalmanState,     // 7 estados × 7 observaciones
    age: u32,                // frames desde creación
    hits: u32,               // detecciones consecutivas (streak)
    misses: u32,             // frames consecutivos sin match (reset a 0 en cada match)
    is_confirmed: bool,       // hits >= min_hits
    first_seen_at: Instant,
    last_seen_at: Instant,
    source_models: Vec<String>, // qué modelos contribuyeron a este track
}

enum TrackEvent {
    Created  { track_id: u64, class: String, bbox: [f32; 4] },
    Updated  { track_id: u64, bbox: [f32; 4] },
    Lost     { track_id: u64, last_bbox: [f32; 4] },
    Deleted  { track_id: u64, reason: String },  // "age_exceeded" | "zone_exited"
}
```

**Algoritmo por frame:**

```
fn update(&mut self, detections: &[Detection]) -> Vec<TrackEvent> {
    let mut events = Vec::new();

    // Paso 1: PREDECIR
    for track in self.tracks.values_mut() {
        track.kalman.predict();
        track.bbox = kalman_to_bbox(&track.kalman);  // x1y1x2y2 desde estado
    }

    // Paso 2: MATCHING (Hungarian sobre matriz de costos IoU 1-IoU)
    let active_tracks: Vec<u64> = self.tracks.keys().copied().collect();
    let cost_matrix = build_iou_cost_matrix(&active_tracks, &self.tracks, detections);
    let (matched, unmatched_tracks, unmatched_dets) = hungarian(&cost_matrix, self.iou_threshold);

    // Paso 3: UPDATE tracks matched
    for (track_id, det_idx) in matched {
        let track = &mut self.tracks.get_mut(&track_id).unwrap();
        track.kalman.update(bbox_to_measurement(&detections[det_idx].bbox));
        track.bbox = detections[det_idx].bbox;
        track.hits += 1;
        track.misses = 0;
        track.last_seen_at = Instant::now();
        track.is_confirmed = track.hits >= self.min_hits;
        events.push(TrackEvent::Updated { track_id, bbox: track.bbox });
    }

    // Paso 4: CREATE tracks from unmatched detections
    for det_idx in unmatched_dets {
        let track_id = self.next_id;
        self.next_id += 1;
        let det = &detections[det_idx];
        self.tracks.insert(track_id, TrackState {
            id: track_id,
            class: det.class.clone(),
            bbox: det.bbox,
            kalman: KalmanState::from_bbox(&det.bbox),
            age: 1, hits: 1, misses: 0,
            is_confirmed: false,
            first_seen_at: Instant::now(),
            last_seen_at: Instant::now(),
            source_models: vec![det.source_model.clone()],
        });
        events.push(TrackEvent::Created { track_id, class: det.class.clone(), bbox: det.bbox });
    }

    // Paso 5: AGE unmatched tracks
    for track_id in unmatched_tracks {
        let track = &mut self.tracks.get_mut(&track_id).unwrap();
        track.misses += 1;
        track.age += 1;
        if track.misses > self.max_age {
            let reason = if track.is_confirmed { "age_exceeded" } else { "unconfirmed" };
            self.tracks.remove(&track_id);
            events.push(TrackEvent::Deleted { track_id, reason: reason.to_string() });
        } else if track.is_confirmed {
            events.push(TrackEvent::Lost { track_id, last_bbox: track.bbox });
        }
    }

    events
}
```

## Kalman filter (7D)

```
Estado:     x = [cx, cy, s, r, dcx, dcy, ds]  (7×1)
Medición:   z = [cx, cy, s, r]                  (4×1)

cx, cy = centro del bbox
s      = escala (area w*h)
r      = aspect ratio (w/h)
dcx, dcy, ds = velocidades (derivadas)
```

- Transición: modelo de velocidad constante (linear motion)
- Observación: medición directa de posición + escala + ratio
- Inicialización: estado desde primer bbox, velocidades = 0, covarianza inicial alta en velocidad (incertidumbre)

**Simplificación para escenas clínicas:**

Las personas en habitaciones clínicas no hacen movimientos bruscos. El modelo de velocidad constante es adecuado. A diferencia de escenas de tráfico (MOTChallenge), aquí:
- Movimientos son lentos (caminar, sentarse, acostarse)
- Oclusiones son por muebles (cama, silla), no por otros objetos en movimiento
- Los tracks pueden ser estáticos por minutos (paciente durmiendo)

## Hungarian algorithm (linear assignment)

Matriz de costos N×M donde `cost[i][j] = 1 - IoU(track_i.bbox, detection_j.bbox)`. La implementación es el algoritmo húngaro clásico (Kuhn-Munkres) O(n³). Para n≤20 tracks clínicos, el costo es despreciable (< 0.5ms).

**Alternativa:** greedy matching (asignar cada track a la detección más cercana, una por una). Más rápido O(n²) pero subóptimo — puede producir matches peores. Hungarian da el matching global óptimo.

## Why SORT over DeepSORT?

| Aspecto | SORT | DeepSORT |
|---------|------|----------|
| Features | Solo bbox (4D) | Bbox + appearance embedding (128D) |
| Re-identification | No (pierde track en oclusión larga) | Sí (re-identifica por apariencia) |
| Complejidad | 250 líneas Rust | 400+ líneas + modelo de embeddings |
| Dependencias | Ninguna (solo nalgebra) | ONNX model for ReID |
| Precisión clínica | Suficiente (oclusiones cortas < 1s) | Overkill |

En una habitación clínica, las oclusiones duran segundos como máximo (enfermera pasa frente a cámara). SORT maneja oclusiones de hasta ~10 frames (1-2 segundos a 5-10fps de i-frames). Para oclusiones más largas, el track se pierde y se crea uno nuevo — clínicamente aceptable porque la persona no cambió (misma clase, misma zona).

## Tuning para escenas clínicas

| Parámetro | Default | Clínico | Razón |
|-----------|---------|---------|-------|
| `max_age` | 10 | 20 | I-frames cada 2s → 20 frames = 40s de oclusión tolerada |
| `min_hits` | 3 | 2 | Confirmar rápido (clínico no puede esperar 3 i-frames = 6s) |
| `iou_threshold` | 0.3 | 0.2 | Personas lejanas = bboxes pequeños = IoU más bajo en matching |

Estos valores son defaults configurables desde `mana.toml` en el futuro.

## Consequences

- **Positive:** Identidad persistente permite eventos con sujeto ("track 1 salió de la cama").
- **Positive:** Filtrado natural de falsos positivos (detecciones esporádicas → tracks unconfirmed → eliminados).
- **Positive:** Algoritmo bien conocido, implementaciones de referencia en Python/C++ fácilmente portables a Rust.
- **Negative:** Sin re-identificación por apariencia. Si dos personas intercambian posición durante una oclusión, los tracks se intercambian. Clínicamente inusual.
- **Negative:** Kalman necesita nalgebra o implementación manual de álgebra lineal. ~150 líneas para Kalman 7D.
- **Negative:** Hungarian O(n³) es aceptable para n≤20. Si hay 100+ detecciones (multitud), necesitamos cascaded matching o greedy fallback.

## References

- Bewley, A. et al. "Simple Online and Realtime Tracking." ICIP 2016. [arXiv:1602.00763](https://arxiv.org/abs/1602.00763)
- [SORT Python reference](https://github.com/abewley/sort)
- ADR-012: Postprocess Pipeline (produce Vec<Detection>)
- ADR-014: Zone Engine (consume Active Tracks)
