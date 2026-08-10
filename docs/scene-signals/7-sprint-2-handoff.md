# Handoff — Sprint 2: producir señales en paralelo

**Estado:** Etapa B implementada; siguiente hito: Etapa C
**Precondición:** Sprint 1 / Etapa A cerrado funcionalmente
**Fuente operativa:** [tasks.md](tasks.md#7-sprint-2--entrada-y-primer-objetivo)

Este documento es la entrada de una sesión nueva. Resume qué quedó cerrado, qué
se puede tocar y cuál es la primera entrega de B. No reemplaza el contrato ni
el diseño técnico.

La evidencia de cierre está en [8-sprint-2-cierre.md](8-sprint-2-cierre.md).

## Lectura rápida

1. [1-spec.md](1-spec.md) — contrato normativo.
2. [design.md](design.md) — arquitectura y secuencia de implementación.
3. [2-sprints.md](2-sprints.md#etapa-b--producir-en-paralelo) — compuerta de B.
4. [6-sprint-1-cierre.md](6-sprint-1-cierre.md) — evidencia de A.
5. Este handoff — contexto operativo para la sesión.

## Estado de entrada

Sprint 1 dejó en `mana-control` un catálogo estático v1, valores tipados,
operadores compatibles, una tabla ordenada y un snapshot inmutable. La tabla
todavía no participa en la decisión clínica.

La fuente de verdad de los guards sigue siendo `FsmSceneContext`. La Etapa B
produce la tabla en paralelo precisamente para demostrar paridad antes de que
un guard la lea.

La evidencia de cierre está en [6-sprint-1-cierre.md](6-sprint-1-cierre.md):

- `cargo test -p mana-control`: 148 tests en debug.
- `cargo test -p mana-control --release`: 147 tests; el test restante es sólo `cfg(debug_assertions)`.
- `cargo test --workspace` y release: correctos.
- Goldens sin cambios.
- `cargo fmt --all -- --check` y `git diff --check`: correctos.

La compuerta estricta de Clippy del workspace continúa fallando por deuda
preexistente fuera de `signals/`. No ampliar Sprint 2 para resolver esa deuda.

El worktree contiene cambios no commiteados del proyecto y artefactos ajenos:

- `.kiro/` contiene borradores anteriores y no es fuente normativa.
- `.claude/worktrees/` contiene worktrees auxiliares y no pertenece al cierre.
- No hacer `git add -A`, no limpiar esos directorios y no revertir cambios ajenos.

## Objetivo de B

En cada ciclo, materializar las ocho señales base desde la misma evidencia que
actualiza `FsmSceneContext`, y publicar la novena señal desde el latch de
`FsmEngine`. La tabla no gobierna ningún guard durante esta etapa.

El resultado buscado es una afirmación comprobable por tick:

```text
valor de FsmSceneContext <=> valor equivalente en SignalTable
```

La única diferencia permitida es la representación explícita de ausencia. No
se rellena una ausencia con `false`, cero, un label inventado ni el valor del
ciclo anterior.

## Catálogo que se produce

| Evidencia actual | Tag | Tipo | Presencia |
|---|---|---|---|
| `raw_person_count > 0` | `persona.presente` | `Bool` | siempre |
| `raw_person_count` | `persona.cantidad` | `Count` | siempre |
| cara seleccionada | `cara.presente` | `Bool` | siempre |
| confianza de la cara seleccionada | `cara.confianza` | `Ratio` | sólo con cara |
| `face_in_dwell` | `cara.en_dwell` | `Bool` | sólo con ROI de dwell |
| `at_edge` | `cara.en_borde` | `Bool` | siempre |
| `face_model_ran` | `cara.modelo_corrio` | `Bool` | siempre |
| cardinalidad actual | `ocupacion.cardinalidad` | `Label` | siempre |
| latch `face_was_inside` | `cara.estuvo_dentro` | `Bool` | mientras FSM esté activa |

`cara.en_dwell = ausente` significa que no hay ROI configurado. `false`
significa que la ROI existe y la observación fue negativa. Son estados
distintos y deben aparecer así en la prueba de paridad.

## Secuencia obligatoria del tick

1. `scan()` congela la `ProcessImage` y calcula el instante de control.
2. El lazo actualiza presencia, tracking, ocupación y zonas como hoy.
3. `update_context()` actualiza `FsmSceneContext` y produce las ocho señales base.
4. `FsmEngine` aplica la lógica existente de `face_was_inside`.
5. La tabla recibe `cara.estuvo_dentro` después del latch y antes de la evaluación normal.
6. La tabla queda sin lectores productivos; los guards siguen leyendo el contexto.
7. El lote de eventos y los goldens permanecen iguales.

La tabla debe ser nueva por ciclo. No agregar `clear()` como mecanismo de
reutilización y no arrastrar señales del tick anterior.

## Primeras tareas

### S2-00 — Congelar entrada

Registrar `git status --short --branch`, ejecutar las suites de referencia y
confirmar que `tests/golden/` no tiene cambios antes de tocar `scan()`.

### S2-01 — Producción base

Conectar `SignalTable` al punto de actualización de contexto. Insertar las ocho
señales con los valores ya calculados, validando cada inserción contra
`scene_signal_catalog()`.

No introducir guards, TOML, `SceneEvent`, logger ni `SignalFault` todavía.

### S2-02 — Latch derivado

Publicar `cara.estuvo_dentro` sólo después de aplicar el latch existente. No
duplicar la lógica del latch en `scan.rs` ni modificar sus reglas clínicas.

### S2-03 — Paridad

Extender la cobertura de `multi_actor_cycle` para comprobar los nueve tags en
cada tick relevante. Debe cubrir cara presente, confianza, dwell, borde,
cardinalidad múltiple, ausencia de dwell y el latch.

### S2-04 — Ciclos independientes

Demostrar que dos tablas consecutivas no comparten valores y que
`cara.en_dwell` ausente no se transforma en `Bool(false)`.

### S2-05 — Compuerta B

Ejecutar las suites debug y release, revisar el diff de goldens y comprobar que
la tabla sigue sin lectores en FSM, logger o configuración.

## Archivos esperados

La implementación probablemente tocará:

- `core/mana-control/src/scan.rs`
- `core/mana-control/src/fsm/engine.rs`
- pruebas unitarias de `scan` o `fsm`
- una prueba de paridad sobre el escenario `multi_actor_cycle`
- `docs/scene-signals/tasks.md` para marcar S2 sólo cuando la evidencia exista

Si agregar un campo a `ControlState` rompe constructores públicos, actualizar
los constructores explícitamente y revisar todos los usos reales. No ocultar la
tabla en un cache global ni convertirla en autoridad de transición por
comodidad.

## Decisiones que no se reabren

- El catálogo v1 tiene nueve tags, no ocho.
- `BTreeMap` es deliberado: la observabilidad futura necesita orden estable.
- `Ratio` no admite igualdad exacta.
- Ausencia no equivale a `false` y tampoco coincide con `!=`.
- `FsmSceneContext` y `ProgramGuard` no se fusionan.
- Zonas, Health y profundidad conservan sus motores especializados.
- No se cambian thresholds, dwell, prioridades, wildcards ni goldens.
- No se agregan reglas en caliente.

## Compuerta de B

```sh
cargo test --workspace
cargo test --workspace --release
cargo fmt --all -- --check
git diff --check
git diff tests/golden/
```

Debe ser cierto al cerrar B:

- La paridad tick a tick está probada.
- Las nueve señales se producen con la semántica del catálogo.
- Una tabla nueva no hereda valores.
- La tabla no tiene lectores productivos.
- Los goldens siguen byte-idénticos.

El warning global de Clippy debe registrarse, no esconderse con un filtro que
pueda ocultar nuevos warnings de `signals/`.

## Primer comando de la sesión nueva

```sh
git status --short --branch
```

Después leer este archivo, `tasks.md` y la sección B de `2-sprints.md` antes de
editar código.
