# Cierre — Etapa B: producir señales en paralelo

**Estado:** Etapa B cerrada funcionalmente; Etapa C pendiente
**Entrada:** [7-sprint-2-handoff.md](7-sprint-2-handoff.md)
**Plan:** [2-sprints.md](2-sprints.md#etapa-b--producir-en-paralelo)

## Entrega

- `scan()` crea una `SignalTable` nueva en cada tick.
- `update_context()` mantiene `FsmSceneContext` y produce las ocho señales base.
- `ControlState::signal_snapshot` conserva el snapshot inmutable del ciclo más
  reciente.
- `FsmEngine` aplica sus reglas existentes de `face_was_inside` antes de que la
  señal derivada `cara.estuvo_dentro` entre al snapshot.
- La FSM y los guards continúan leyendo únicamente `FsmSceneContext`.
- No se agregaron lectores productivos, `SceneEvent`, logger, TOML ni guards
  genéricos.

## Evidencia

- `cargo test --workspace`: 150 tests correctos.
- `cargo test --workspace --release`: 149 tests correctos; el test restante es
  sólo `cfg(debug_assertions)`.
- `cargo fmt --all -- --check`: correcto.
- `git diff --check`: correcto.
- `git diff tests/golden/`: vacío.
- `multi_actor_cycle` verifica paridad de los nueve tags en cada tick relevante.
- Las pruebas de `scan` verifican ausencia de confianza, `cara.en_dwell = false`
  con ROI configurada y tablas independientes entre ciclos.

La compuerta estricta de Clippy continúa bloqueada por warnings preexistentes
fuera de `mana-control`, principalmente en `std/mana-geometry`. No se ocultó esa
deuda con filtros.

## Siguiente etapa

Etapa C puede comenzar con `FsmGuard::Signal`, validación contra el catálogo y
la migración individual de los guards simples. Zonas, Health y profundidad
siguen fuera de esa migración.
