# Cierre — Sprint 1: vocabulario de señales

**Fecha:** 2026-08-10
**Resultado:** Etapa A cerrada funcionalmente; Etapa B abierta.

## Entrega

- `SignalTag` pertenece a `mana-control` y usa `domain_id!` sin `Copy`.
- El catálogo v1 declara nueve tags, versión `1`, tipos, presencia y labels.
- `Ratio` rechaza valores fuera de rango y no expone el `f32` fuera del crate.
- `SignalValue` y `SignalOp` aplican la matriz tipada sin coerciones.
- `SignalTable` usa `BTreeMap`, valida inserciones y conserva ausencia distinta de `false`.
- `SceneSignalsSnapshot` itera todos los tags del catálogo en orden determinista.
- No hay productores ni consumidores en `scan`, FSM, logger, blueprints o fixtures.

## Evidencia

| Comando | Resultado |
|---|---|
| `cargo test -p mana-control` | PASS — 148 tests |
| `cargo test -p mana-control --release` | PASS — 147 tests; un test `cfg(debug_assertions)` queda fuera |
| `cargo test --workspace` | PASS |
| `cargo test --workspace --release` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS |
| `git diff tests/golden/` | vacío |
| `rg -n "SignalTable" core/mana-control/src` | sólo `signals/` y sus pruebas |

## Excepción de línea base

`cargo clippy --workspace -- -D warnings` fue ejecutado, pero no queda verde
por warnings preexistentes en `std/mana-geometry`, módulos legacy de
`mana-control` y el binario principal. La salida no reporta warnings en
`core/mana-control/src/signals/`. No se tocaron esos módulos para cerrar este
sprint; queda como deuda separada del proyecto de señales.

## Alcance siguiente

Sprint 2 comienza con producción paralela: las ocho señales base desde
`update_context()`, luego `cara.estuvo_dentro` desde el latch de `FsmEngine`,
paridad tick a tick y ningún consumidor genérico todavía.

`.kiro/` conserva borradores anteriores y `.claude/worktrees/` son worktrees
auxiliares. Ninguno forma parte del cierre de este sprint.
