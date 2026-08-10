# Handoff — Etapa D: el gemelo visible

**Estado:** Etapa C cerrada; siguiente hito: Etapa D
**Precondición:** Etapas A, B y C cerradas funcionalmente
**Branch de entrada:** `dev`
**Commit de entrada:** `c6902da docs(signals): cerrar etapa C`
**Fuente operativa:** [tasks.md](tasks.md#8-sprint-3--guard-genérico)

Este documento es la entrada de una sesión nueva para implementar D. No reemplaza
el contrato ni el diseño técnico. La evidencia de C está en
[9-sprint-3-cierre.md](9-sprint-3-cierre.md).

## Lectura rápida

1. [1-spec.md](1-spec.md) — contrato normativo y requisitos RF-06/RF-07.
2. [design.md](design.md#11-observabilidad-y-gemelo-digital) — forma del evento y serialización.
3. [5-engine-funcional.md](5-engine-funcional.md#observabilidad-del-gemelo-digital) — posición en T2/T3.
4. [2-sprints.md](2-sprints.md#etapa-d--el-gemelo-visible) — alcance y compuerta de D.
5. [9-sprint-3-cierre.md](9-sprint-3-cierre.md) — estado de entrada y decisiones cerradas.
6. Este handoff — orden operativo para la sesión.

## Estado de entrada

El worktree de `dev` está limpio. A, B y C están cerradas; D todavía no tiene
implementación. La compuerta de entrada quedó verde en debug, release y la
variante `ffmpeg` sin defaults. Los goldens existentes son la línea de base y no
se regeneran sin una prueba que explique cada línea nueva.

La compuerta estricta de Clippy continúa bloqueada por warnings preexistentes,
principalmente en `std/mana-geometry`. Esa deuda no pertenece a D y no se debe
resolver mezclándola con observabilidad.

No hacer `git add -A`. No tocar ni limpiar `.kiro/`, `.claude/worktrees/` ni
otros artefactos locales. No revertir cambios ajenos si aparecen durante la
sesión.

## Objetivo de D

Al terminar D, un incidente debe poder responder con datos del mismo ciclo:

1. Qué señales estaban presentes.
2. Qué señales declaradas estaban ausentes.
3. Qué sello de control correlaciona la evidencia con el frame y sus edades.
4. Qué transición produjo la FSM, o qué evidencia explica que no coincidiera.

La entrega es aditiva. D agrega observabilidad; no cambia thresholds, dwell,
prioridad, orden de transiciones, wildcards, Health, zonas ni profundidad.

## Arquitectura actual

La tabla ya existe y gobierna los guards en C:

- `core/mana-control/src/scan.rs` materializa una `SignalTable` nueva por tick.
- `ControlState::signal_snapshot` conserva el snapshot más reciente.
- `SceneSignalsSnapshot::iter()` expone las nueve entradas en orden estable,
  incluyendo `None` para ausencias.
- `scan()` devuelve `SceneEvent`, pero todavía no tiene `SceneSignals`.
- `src/logger/event/scene.rs` descarta deliberadamente `Occupancy` y `FsmState`
  y todavía no conoce señales.
- `src/logger/event/mod.rs` todavía no tiene `Event::SceneSignals`.
- `src/logger/serialize/control.rs` contiene la serialización de eventos de
  control y es el lugar natural para el nuevo registro JSONL.
- `src/app/mod.rs` llama `scene_events_to_log()` después de `scan()` y emite el
  `FaceDwell` diagnóstico desde `FsmSceneContext` y `FsmSnapshot`.
- `src/face_dwell.rs` todavía depende del contexto plano y debe migrarse antes
  de retirar ese contexto.

Hay una frontera importante: T2 produce el evento, pero T2 no serializa, no hace
I/O y no conoce sinks. La conversión a `Event` y el JSONL ocurren después del
batch, en T3, con degradación best-effort.

## Secuencia obligatoria

### D-00 — Congelar entrada

Ejecutar antes de editar:

```sh
git status --short --branch
git log -5 --oneline --decorate
cargo test --workspace
cargo test --workspace --release
git diff master...HEAD -- tests/golden/
```

No usar `UPDATE_GOLDEN=1` en esta etapa de entrada.

### D-01 — Evento de dominio

Agregar al contrato de `SceneEvent`:

```rust
SceneEvent::SceneSignals {
    stamp: ControlStamp,
    snapshot: SceneSignalsSnapshot,
}
```

Emitir exactamente una vez por `scan()`, usando el `ControlStamp` del mismo
input. La emisión debe ocurrir después de construir `state.signal_snapshot` y
antes de la evaluación normal de la FSM, de modo que el snapshot observado sea
exactamente el que leen los guards.

La emisión también debe existir cuando no hay `FsmEngine`: en ese caso
`cara.estuvo_dentro` permanece ausente, no se inventa `false`.

Probar el orden bruto del batch en `multi_actor_cycle.events.txt`. El evento no
puede duplicarse por la pasada wildcard ni por una transición.

### D-02 — Evento de logger

Agregar `Event::SceneSignals` y conectarlo a:

- constructor en `src/logger/event/constructors.rs`;
- mapper en `src/logger/event/scene.rs`;
- filtro en `MetricsJsonlConfig::allows()`;
- nivel mínimo informativo en `Event::min_level()`;
- dispatcher de `src/logger/serialize/mod.rs`;
- serializador de control, preferentemente junto a
  `write_face_dwell_event()` en `src/logger/serialize/control.rs`.

El evento debe persistir por defecto. No esconderlo detrás de un flag de
diagnóstico que lo deje fuera del log informativo normal.

La forma JSON debe incluir el `ControlStamp` existente, `catalog_version = 1` y
las nueve señales en orden determinista. Cada entrada incluye su tipo y:

- `value` si está presente;
- `absent: true` si está ausente.

No usar un `HashMap` de orden arbitrario ni inventar `tick`, `timestamp_ms` o un
contador paralelo. Reutilizar `write_control_stamp()` y los escritores seguros
existentes.

`Ratio::get()` es actualmente crate-private. Si el serializador necesita el
valor numérico, abrir una API pública de lectura que no exponga igualdad exacta
ni permita construir ratios inválidos; no duplicar el campo privado ni usar
conversión textual de `Debug`.

### D-03 — Best effort y sink

Verificar que una falla de logger o sink no impide completar T2 ni altera la
decisión del ciclo. El snapshot debe ser un dato ya construido antes de entrar
al mapper; ninguna serialización debe ejecutarse dentro de `mana-control`.

Agregar una prueba de serialización con valores y ausencias, y una prueba de
degradación del sink siguiendo el patrón de `src/logger/tests.rs`.

### D-04 — Migrar `FaceDwellLogStrategy`

`src/face_dwell.rs` todavía lee `FsmSceneContext`. Antes de retirarlo, hacer que
la estrategia consuma `SceneSignalsSnapshot` para cardinalidad, presencia,
confianza, dwell, borde, latch y modelo facial. El `FsmSnapshot` sigue aportando
estado, dwell de estado y timers de la FSM.

La señal de auditoría completa y el evento legado `FaceDwell` pueden coexistir
durante D. No eliminar el evento legado hasta demostrar que el nuevo snapshot
cubre la necesidad de diagnóstico y que los tests existentes siguen explicando
el ciclo.

### D-05 — Retirar el contexto plano

Sólo después de D-04, retirar gradualmente `FsmSceneContext` como struct de
campos de escena de `ControlState`, App y logger. El latch `face_was_inside` y
la lógica temporal siguen perteneciendo al engine; no moverlos a un segundo
motor ni al logger.

Actualizar explícitamente constructores, tests, viz y cualquier consumidor real.
No dejar un contexto paralelo que vuelva a ser fuente de verdad por comodidad.

### D-06 — Goldens y correlación

Extender `tests/golden_multi_actor_cycle.rs` para comprobar por tick:

- nueve tags declarados;
- valores presentes correctos;
- ausencias explícitas, especialmente confianza sin cara y dwell sin ROI;
- `scan_seq`, frame y edades iguales al `ControlStamp` del ciclo;
- un único evento `SceneSignals` por scan.

Actualizar `tests/golden/multi_actor_cycle.events.txt` para el batch bruto y el
JSONL sólo después de que las aserciones de contrato estén escritas.

El golden anterior debe ser prefijo del nuevo, o una comparación equivalente
que demuestre que las únicas diferencias son los eventos de observabilidad. Si
la inserción antes de la transición rompe el prefijo textual, no se regenera la
línea de base: revisar el punto de mapeo para conservar la secuencia legacy y
agregar la señal de forma aditiva.

## Archivos esperados

La implementación probablemente tocará:

- `core/mana-control/src/scan.rs`;
- `core/mana-control/src/signals/value.rs` o una API de serialización del crate;
- `src/logger/event/mod.rs`;
- `src/logger/event/constructors.rs`;
- `src/logger/event/scene.rs`;
- `src/logger/serialize/mod.rs`;
- `src/logger/serialize/control.rs`;
- `src/logger/mod.rs`;
- `src/face_dwell.rs`;
- `src/app/mod.rs`;
- `tests/golden_multi_actor_cycle.rs` y sus fixtures;
- `src/logger/tests.rs`;
- documentación de cierre de D.

## Decisiones que no se reabren

- El catálogo v1 tiene nueve tags y el snapshot incluye los nueve, presentes o
  ausentes.
- `BTreeMap` y el recorrido ordenado son parte del contrato observable.
- Ausencia no equivale a `false`, cero, label inventado ni valor anterior.
- `SceneEvent::SceneSignals` usa `ControlStamp`; no crea relojes ni contadores.
- El logger es T3 best-effort y nunca participa en una decisión clínica.
- Zonas, Health y profundidad conservan guards especializados.
- No hay hot reload, tags libres, protocolo externo, compresión ni política de
  retención en esta etapa.
- No se regeneran goldens para ocultar una alteración de comportamiento.

## Compuerta de D

```sh
cargo test --workspace
cargo test --workspace --release
cargo fmt --all -- --check
git diff --check
git diff tests/golden/
```

Debe ser cierto al cerrar D:

- Cada scan emite un solo snapshot completo con los nueve tags.
- La serialización conserva valores, ausencias, orden y `ControlStamp`.
- El evento aparece en JSONL con nivel informativo y persistencia por defecto.
- Una falla de sink no bloquea T2 ni modifica el resultado de FSM.
- `FaceDwellLogStrategy` ya no depende del contexto plano antes de retirarlo.
- El golden anterior es prefijo del nuevo o la comparación demuestra que sólo se
  agregaron líneas de observabilidad.
- No cambió ninguna decisión clínica.

La compuerta estricta de Clippy debe registrarse como deuda de línea base, sin
ocultar warnings nuevos con filtros globales.

## Primer comando de la sesión nueva

```sh
git status --short --branch
```

Después leer este archivo, `9-sprint-3-cierre.md`, `design.md` §11 y la sección
D de `2-sprints.md` antes de editar código.
