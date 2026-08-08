# ADR-015: FSM Engine — Guard Evaluation, Dwell Timers, Ton/Tof

**Status:** Accepted
**Date:** 2026-08-04

## Context

La máquina de estados finitos (FSM) es el cerebro clínico de Mana Lite. Evalúa guards contra el estado actual de zonas y tracks, aplica timers de dwell, y emite transiciones que el logger serializa como eventos clínicos.

El diseño debe ser determinista (mismas entradas → misma transición), verificable (un clinical engineer puede leer `fsm.toml` y entender la lógica), y robusto (no debe oscilar entre estados).

## Decision

**Evaluación por orden de prioridad: wildcards primero, luego transiciones explícitas del estado actual, con dwell timers acumulativos.**

```rust
struct FsmEngine {
    config: FsmCatalog,
    current_state: String,
    state_entered_at: Instant,           // cuándo entramos al estado actual
    dwell_counters: HashMap<String, Instant>,  // trigger → cuándo empezó a evaluar true
    dwell_started: HashMap<String, Instant>,
}

struct FsmTransitionResult {
    from: String,
    to: String,
    trigger: String,
    dwell_ms: u64,
}
```

**Algoritmo:**

```rust
impl FsmEngine {
    fn evaluate(
        &mut self,
        zone_events: &[ZoneEvent],       // eventos de zona de este ciclo
        tracks: &HashMap<u64, TrackState>,
        health: &Health,                  // para guard data_stale
    ) -> Option<FsmTransitionResult> {
        // 1. Evaluar transiciones wildcard (from = "*") — aplican desde cualquier estado
        for transition in &self.config.fsm.transitions {
            if transition.from != "*" { continue; }
            if let Some(result) = self.eval_transition(transition, zone_events, tracks, health) {
                return Some(result);
            }
        }

        // 2. Evaluar transiciones desde el estado actual
        for transition in &self.config.fsm.transitions {
            if transition.from != self.current_state { continue; }
            if let Some(result) = self.eval_transition(transition, zone_events, tracks, health) {
                return Some(result);
            }
        }

        // 3. Evaluar dwell del estado actual (auto-escalación por tiempo)
        if let Some(state) = self.config.fsm.states.get(&self.current_state) {
            if let Some(dwell_min) = state.dwell_min_ms {
                let elapsed = self.state_entered_at.elapsed().as_millis() as u64;
                if elapsed >= dwell_min {
                    // Buscar transición con dwell match desde este estado
                    for transition in &self.config.fsm.transitions {
                        if transition.from == self.current_state
                            && transition.guards.is_empty()
                            && transition.dwell.is_some()
                        {
                            return Some(FsmTransitionResult {
                                from: self.current_state.clone(),
                                to: transition.to.clone(),
                                trigger: "dwell_timeout".into(),
                                dwell_ms: elapsed,
                            });
                        }
                    }
                    // Si no hay transición de dwell definida, no hacer nada
                    // (el estado actual no tiene escape automático — se queda aquí)
                }
            }
        }

        None  // sin transición este ciclo
    }
}
```

## Evaluación de guards

Cada guard tiene tipo y parámetros. La evaluación es booleana:

```rust
fn eval_guard(&self, guard: &FsmGuard, zone_events: &[ZoneEvent],
              tracks: &HashMap<u64, TrackState>, health: &Health) -> bool {
    match guard {
        FsmGuard::ZoneOccupied { zone, min_confidence, min_duration_ms } => {
            // ¿Hay un Occupied event para esta zona en este ciclo?
            zone_events.iter().any(|ev| match ev {
                ZoneEvent::Occupied { zone: z, .. } => z == zone,
                _ => false,
            })
        }
        FsmGuard::ZoneVacated { zone, min_duration_ms } => {
            // ¿Hay un Vacated event para esta zona?
            zone_events.iter().any(|ev| match ev {
                ZoneEvent::Vacated { zone: z, .. } => z == zone,
                _ => false,
            })
        }
        FsmGuard::AllZonesVacant { min_duration_ms } => {
            // ¿Todas las zonas reportaron vacated?
            // Usar ZoneEngine::all_vacant()
            true  // placeholder — ZoneEngine expone esta info
        }
        FsmGuard::DataStale => {
            // ¿Health está en estado blind?
            health.is_blind()
        }
    }
}
```

## Dwell timers (Ton)

Cuando una transición tiene guards, estos deben evaluar `true` **consistentemente** por `min_duration_ms` antes de disparar. Si en cualquier frame intermedio el guard evalúa `false`, el contador se resetea.

```rust
fn eval_transition_with_dwell(&mut self, transition: &FsmTransition, ...) -> Option<FsmTransitionResult> {
    let guard_key = format!("{}→{}", transition.from, transition.to);

    if transition.guards.is_empty() {
        // Sin guards → transición inmediata (dwell no aplica)
        return Some(FsmTransitionResult { ... });
    }

    let all_guards_true = transition.guards.iter()
        .all(|g| self.eval_guard(g, zone_events, tracks, health));

    if all_guards_true {
        let started = self.dwell_counters
            .entry(guard_key.clone())
            .or_insert(Instant::now());
        let elapsed = started.elapsed();

        // Verificar si el dwell mínimo de algún guard se cumple
        let min_dwell: u64 = transition.guards.iter()
            .filter_map(|g| match g {
                FsmGuard::ZoneOccupied { min_duration_ms, .. } => *min_duration_ms,
                FsmGuard::ZoneVacated { min_duration_ms, .. } => *min_duration_ms,
                FsmGuard::AllZonesVacant { min_duration_ms, .. } => *min_duration_ms,
                _ => None,
            })
            .max()  // el dwell más restrictivo
            .unwrap_or(0);

        if elapsed.as_millis() as u64 >= min_dwell {
            self.dwell_counters.remove(&guard_key);
            return Some(FsmTransitionResult {
                from: transition.from.clone(),
                to: transition.to.clone(),
                trigger: guard_key,
                dwell_ms: elapsed.as_millis() as u64,
            });
        }
    } else {
        // Guard no se cumple → resetear contador
        self.dwell_counters.remove(&guard_key);
    }

    None
}
```

## Prioridad de transiciones

Si múltiples transiciones desde el mismo estado tienen guards satisfechos, ¿cuál dispara?

**Decisión:** La primera en orden de definición en `fsm.toml`. Esto es determinista y predecible. El clinical engineer controla la prioridad ordenando las transiciones en el archivo.

Ejemplo:
```toml
[[fsm.transitions]]
from = "watching"
to = "bed_alert"
guards = [{ type = "zone_vacated", zone = "bed", min_duration_ms = 3000 }]
# Esta transición se evalúa primero → si bed vacía por 3s, dispara

[[fsm.transitions]]
from = "watching"
to = "idle"
guards = [{ type = "all_zones_vacant", min_duration_ms = 60000 }]
# Esta se evalúa después → si TODO vacío por 60s, dispara
```

## Wildcard transitions (from = "*")

Estas transiciones aplican desde cualquier estado y se evalúan **antes** que las del estado actual. Útil para condiciones globales como `data_stale → blind`.

```toml
[[fsm.transitions]]
from = "*"
to = "blind"
guards = [{ type = "data_stale" }]
```

Cuando `data_stale` es true, esta transición dispara inmediatamente sin importar si el estado actual es `idle`, `watching` o `bed_alert`. Al volver el frame, el health emite `Recovered` y el FSM puede transicionar de `blind` a un estado normal vía otra transición explícita.

## Estado inicial e histéresis

`fsm.initial` define el estado de arranque. Después, las transiciones son unidireccionales (no hay "volver al estado inicial" automático). Si el FSM llega a un estado sin transiciones de salida, se queda allí permanentemente (ej: `blind` sin recuperación automática requiere intervención externa).

Esto es intencional: estados como `blind` indican que algo grave pasó y requiere atención humana, no recuperación automática.

## Consequences

- **Positive:** Determinista, verificable, predecible. Mismas entradas → misma transición, siempre.
- **Positive:** Wildcards + prioridad por orden de definición da control total al clinical engineer.
- **Positive:** Dwell timers evitan transiciones espurias por ruido de detección momentáneo.
- **Negative:** Sin paralelismo de estados (no hay sub-states ni estados compuestos). Esto es suficiente para lógica clínica lineal. Si en el futuro necesitamos HSM (Hierarchical State Machines), se puede extender.
- **Negative:** Dwell counters no persisten entre reinicios. Si el proceso muere durante un dwell de 60s, el contador se pierde. Para despliegues críticos, agregar persistencia en disco en v0.3.
- **Negative:** Solo una transición por ciclo. Si dos transiciones disparan simultáneamente, solo la primera se aplica. Esto puede causar "starvation" de la segunda. Mitigado por el hecho de que las transiciones clínicas son mutuamente excluyentes (no se puede estar entrando a bed_alert y a idle al mismo tiempo).

## References

- ADR-014: Zone Engine (produce ZoneEvents)
- ADR-013: SORT Tracking (produce Active Tracks)
- `config/fsm.toml` — definición de estados y transiciones
- ADR-003: PLC Superloop (orden de fases: EVALUATE → FSM)
