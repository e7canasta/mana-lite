# ADR-014: Zone Engine — Spatial Evaluation with Hysteresis

**Status:** Draft
**Date:** 2026-08-04

## Context

Las zonas son regiones rectangulares definidas en `zones.toml`. Para cada track activo, necesitamos evaluar si intersecta alguna zona y mantener estado de ocupación con histéresis temporal (evitar oscilaciones ocupado/vacío).

Ejemplo clínico: la zona "bed" debe reportar `vacated` solo si ha estado vacía por >500ms (la persona se levantó realmente, no fue un frame donde la detección falló por oclusión momentánea).

## Decision

**Evaluación por intersección bbox↔zona con dwell counters Ton/Tof.**

```rust
struct ZoneEngine {
    zones: HashMap<String, ZoneState>,
    config: ZoneCatalog,
}

struct ZoneState {
    definition: ZoneEntry,              // x1,y1,x2,y2, hysteresis_ms
    is_occupied: bool,                  // estado actual
    occupied_by: Vec<u64>,             // track_ids actualmente en la zona
    occupied_since: Option<Instant>,   // cuándo se ocupó (para dwell mínimo)
    vacated_since: Option<Instant>,    // cuándo se vació (para hysteresis)
}

enum ZoneEvent {
    Occupied { zone: String, track_id: u64, class: String, frame_id: u64 },
    Vacated { zone: String, track_id: u64, class: String, frame_id: u64 },
}
```

**Algoritmo por frame:**

```rust
impl ZoneEngine {
    fn evaluate(&mut self, tracks: &HashMap<u64, TrackState>, frame_id: u64) -> Vec<ZoneEvent> {
        let mut events = Vec::new();

        for (zone_name, state) in &mut self.zones {
            // 1. Encontrar tracks que intersectan esta zona ahora
            let current_occupants: Vec<u64> = tracks.iter()
                .filter(|(_, t)| t.is_confirmed)  // solo tracks confirmados
                .filter(|(_, t)| intersects(&t.bbox, &state.definition))
                .map(|(id, _)| *id)
                .collect();

            // 2. Transición: vacía → ocupada
            if !current_occupants.is_empty() {
                if !state.is_occupied {
                    state.is_occupied = true;
                    state.occupied_by = current_occupants;
                    state.occupied_since = Some(Instant::now());
                    state.vacated_since = None;
                    for &track_id in &state.occupied_by {
                        let class = &tracks[&track_id].class;
                        events.push(ZoneEvent::Occupied {
                            zone: zone_name.clone(),
                            track_id,
                            class: class.clone(),
                            frame_id,
                        });
                    }
                }
                continue;
            }

            // 3. Zona vacía ahora — aplicar histéresis
            if state.is_occupied {
                let vacated_at = state.vacated_since.get_or_insert(Instant::now());
                let elapsed = vacated_at.elapsed().as_millis() as u64;
                if elapsed >= state.definition.hysteresis_ms {
                    // Hysteresis satisfecha → transición a vacía
                    state.is_occupied = false;
                    let prev_occupants = std::mem::take(&mut state.occupied_by);
                    state.occupied_since = None;
                    state.vacated_since = None;
                    for track_id in prev_occupants {
                        // track puede ya no existir (murió durante hysteresis)
                        let class = tracks.get(&track_id)
                            .map(|t| t.class.clone())
                            .unwrap_or_else(|| "unknown".into());
                        events.push(ZoneEvent::Vacated {
                            zone: zone_name.clone(),
                            track_id,
                            class,
                            frame_id,
                        });
                    }
                }
            }
        }

        events
    }
}
```

## Intersección bbox ↔ zona

Intersección de dos rectángulos alineados a ejes (AABB-AABB):

```rust
fn intersects(bbox: &[f32; 4], zone: &ZoneEntry) -> bool {
    let (bx1, by1, bx2, by2) = (bbox[0], bbox[1], bbox[2], bbox[3]);
    let (zx1, zy1, zx2, zy2) = (zone.x1 as f32, zone.y1 as f32, zone.x2 as f32, zone.y2 as f32);

    // Intersección no vacía
    bx1 < zx2 && bx2 > zx1 && by1 < zy2 && by2 > zy1
}
```

**Criterio:** Cualquier solapamiento cuenta como "ocupada". No requerimos que el centro del bbox esté dentro de la zona (persona parcialmente en la cama = zona ocupada).

Alternativa considerada: IoU threshold (bbox debe solaparse >50% con la zona). Rechazada porque en escenas reales, una persona acostada en la cama puede tener solo un 20-30% del bbox sobre la zona (la cabeza fuera de la cama, el cuerpo dentro).

## Hysteresis temporal

El parámetro `hysteresis_ms` en `zones.toml` define cuánto tiempo la zona debe estar vacía antes de emitir `Vacated`. Esto es un timer **Tof** (off-delay) del mundo PLC:

```
          occupied ──────────────────────────────────────────
zone     ──────────┐                                       ┌────
                   └───────────────────────────────────────┘
                                                           
event    Occupied──┐                           Vacated─────┐
                   │                                       │
                   │←──── hysteresis_ms ──────────────────→│
                   │         (zona vacía pero sin evento)   │
```

Esto evita:
- Oscilación por detecciones intermitentes (falsos negativos de 1-2 frames)
- Falsos "vacated" cuando la persona se mueve dentro de la zona y el bbox sale momentáneamente
- Ruido de tracking (bbox jitter entre frames)

**Timer Ton** (on-delay) para el futuro: una zona debe estar ocupada por N ms antes de considerarse realmente ocupada. Útil para filtrar falsos positivos (detección espuria en zona). No implementado en v0.2.

## Zona "virtual" (comodín)

Para el guard `all_zones_vacant` del FSM, necesitamos saber si TODAS las zonas están vacías. Esto no es un evento de zona individual, sino una consulta al estado agregado:

```rust
impl ZoneEngine {
    fn all_vacant(&self) -> bool {
        self.zones.values().all(|z| !z.is_occupied)
    }

    fn all_vacant_duration(&self) -> Option<u64> {
        // mínimo de vacated_since entre todas las zonas
        self.zones.values()
            .filter(|z| !z.is_occupied)
            .filter_map(|z| z.vacated_since)
            .map(|t| t.elapsed().as_millis() as u64)
            .min()
    }
}
```

## Multi-track en misma zona

Dos personas pueden estar en la misma zona (ej: enfermera + paciente en "bed"). El engine emite `Occupied` por cada track que entra. El estado `is_occupied` es binario (zona ocupada o no), no por track. Los eventos individuales permiten al FSM y al logger rastrear quién está dónde.

## Consequences

- **Positive:** Hysteresis elimina oscilaciones — sin falsos vacated por detecciones fallidas.
- **Positive:** Evaluación O(zonas × tracks) con <5×20 = 100 chequeos por ciclo. <0.1ms.
- **Positive:** Eventos de zona referencian track_id → trazabilidad completa ("quién salió de la cama").
- **Negative:** `is_occupied` binario no distingue entre 1 o N ocupantes de una zona. Para el FSM clínico esto es suficiente (la cama está ocupada o no). Si en el futuro necesitamos contar ocupantes, extender a `HashMap<u64, OccupantState>`.
- **Negative:** Hysteresis solo se aplica a vacated, no a occupied. Un falso positivo en zona vacía dispara Occupied inmediatamente. Mitigado por `min_hits` del tracker (track debe ser confirmado para contar).

## References

- ADR-013: SORT Tracking (produce Active Tracks)
- ADR-015: FSM Engine (consume ZoneEvents)
- `config/zones.toml` — definición de zonas
