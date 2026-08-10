# Onboarding — Mana Lite

*Estado: post-Sprint 4, 2026-08-10. Si esta fecha quedó vieja, verificá los
números con los comandos de la sección 4 antes de creerles.*

## 1. El modelo mental, en un párrafo

Esto no es una app de visión que además tiene lógica. Es un **PLC cuyo
dispositivo de campo resulta ser una cámara**. Corre a dos tasas: el campo
(RTSP → decode → ONNX) va a la tasa que puede, con latencia variable y fallando
seguido; el programa (tracker → presencia → ocupación → zonas → FSM → health)
corre a **cadencia fija** y tiene que emitir salida en cada tick aunque el campo
esté muerto. Entre ambos hay un solo objeto: la **imagen de proceso**
(`ProcessImage`), que es el gemelo digital — congelado, fechado, con edad
explícita.

Todo lo demás del diseño se deriva de ahí.

## 2. La única pregunta que ubica cualquier cosa

> **Si la entrada nunca vuelve a llegar, ¿esto tiene que seguir produciendo
> salida correcta en cada tick?**
>
> Sí → **T2 programa**. No → **T1 campo**.

No hace falta más criterio. Los cuatro tiers y dónde viven hoy:

| Tier | Qué es | Tasa | Fallar es | Dónde |
|---|---|---|---|---|
| **T0** · álgebra | sin tasa, sin estado, sin reloj | — | imposible | `mana-id`, `mana-geometry` |
| **T1** · campo | sensado y E/S | variable | normal | `mana-media`, `mana-perception` |
| — | **`ProcessImage`** — la frontera | | | pertenece a T2 |
| **T2** · programa | cadencia fija, determinista, reloj inyectado | fija | un bug | `mana-control` |
| **T3** · reporte | JSONL, métricas, Rerun | — | nunca bloquea el tick | el binario (`src/`) |

`ProcessImage` pertenece a T2: el PLC es dueño de su imagen de proceso, los
dispositivos de campo no saben que existe.

**T2 no depende de T1** no por elegancia, sino porque T2 debe seguir corriendo
cuando T1 murió. Lo que lo hace cumplir no es este documento: es que
`core/mana-control/Cargo.toml` tiene exactamente tres dependencias —
`mana-id`, `mana-geometry`, `serde`. Agregar `mana-perception` ahí no rompe una
convención, rompe la compilación.

## 3. Dónde estás hoy

Cuatro sprints cerrados. El estado real, medido:

| | |
|---|---|
| Paquetes | **6**: `mana-lite`, `mana-control`, `mana-perception`, `mana-id`, `mana-geometry`, `mana-media` |
| Tests | **362** en debug, verdes |
| `scan()` | ocho pasos nombrados, ~146 líneas repartidas |
| Reloj en T2 | `ScanInstant` solo nace de `ScanTimeline`; cero `Instant::now()` en producción |
| Vocabulario | newtypes vía `mana-id`; cero `String` en el puerto de control |
| Build | 510 crates con `default`, **264** sin `rerun` |

Lo que está pendiente: **Sprint 3B** — los god files del binario
(`logger/serialize.rs`, `infer/mod.rs`, `logger/mod.rs`, `viz/mod.rs`,
`config/model_loader.rs`, `app/mod.rs`, `app/bootstrap.rs`). Detalle y orden en
[2-sprints.md](2-sprints.md).

## 4. Cómo trabajar acá

### Build rápido

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib"
export CARGO_TARGET_DIR=$HOME/.cache/mana-lite-target
```

`CARGO_TARGET_DIR` importa: cada worktree tiene su propio `target/` y son
decenas de GB cada uno. No va en el repo porque compartir requiere ruta
absoluta, que no es portable.

Si estás tocando T2 y no la visualización, **compilá sin `rerun`**: 264 crates
en vez de 510.

```sh
cargo test --workspace --no-default-features --features ffmpeg
```

`debug` y `release` son dos perfiles: dos compilaciones completas. Eso es
inherente, no hay cache que lo evite.

### La red de seguridad

Tres fixtures, y saber cuál sirve para qué importa:

| Fixture | Qué fija |
|---|---|
| `tests/golden/synthetic_cycle.jsonl` | ciclo de un actor → salida de log |
| `tests/golden/multi_actor_cycle.jsonl` | dos personas, cara, dos zonas → salida de log |
| `tests/golden/multi_actor_cycle.events.txt` | **la secuencia cruda de `SceneEvent` por tick** |

El tercero existe porque los dos primeros **no alcanzan**:
`scene_events_to_log` descarta `SceneEvent::Occupancy` y `SceneEvent::FsmState`,
así que reordenarlos deja los JSONL verdes. Si tocás `scan()`, tu detector es
`events.txt`.

Para regenerar cualquiera: `UPDATE_GOLDEN=1 cargo test <nombre_del_test>`. Y
después mirá el diff — un golden regenerado sin leerlo no es una red, es un
sello de goma.

### Las compuertas

Cada sprint tiene comandos con salida esperada, no criterios. `grep -rn 'logger'
core/mana-control/src → 0` lo verifica cualquiera sin conocer el código.

**Antes de confiar en una compuerta, verificá que falle cuando debe fallar.** No
es paranoia: dos de las compuertas de este repo pasaron en vacío durante sprints
enteros. `git diff tests/golden/` no reportaba nada porque el fixture estaba
sin trackear, y el golden JSONL no detectaba reordenamientos de eventos.

## 5. Lo que cuesta agregar una regla de escena

Es lo que más vas a hacer. Hoy son **6 ediciones en 4 archivos**, todas dentro
de `mana-control`:

| # | Dónde | Qué |
|---|---|---|
| 1 | `fsm/engine.rs` · `FsmSceneContext` | +1 campo |
| 2 | `scan.rs` · `update_context` | +1 línea que lo puebla |
| 3 | `fsm/guard.rs` · `FsmGuard` | +1 variante (cara al TOML) |
| 4 | `fsm/program.rs` · `ProgramGuard` | +1 variante (compilada) |
| 5 | `fsm/program.rs` · `resolve_guard` | +1 brazo de mapeo |
| 6 | `fsm/guard.rs` · `eval_guard` | +1 brazo de evaluación |

Que esté todo adentro de `mana-control` es buena señal: la frontera aguanta.
Pero crece lineal y hay dos enums de 18 variantes que se mantienen sincronizados
a mano.

**El error que no debés cometer:** fusionar `FsmGuard` y `ProgramGuard` para
ahorrar tipeo. Esa duplicación aparente es la separación
compilar-en-boot / ejecutar-determinista de un PLC. `FsmGuard` es el texto del
programa; `ProgramGuard` es el programa compilado y resuelto contra los
catálogos. Preservala.

La salida a futuro —convertir `FsmSceneContext` en una tabla de señales
tipadas— está en [ADR-031](../adrs/031-scene-signal-table.md) y es
**condicional**: no se ejecuta hasta que se dispare un umbral (hoy 18/25
variantes de guard, 7/12 campos de contexto). Mientras tanto, cada booleano
plano que agregues es deuda consciente.

## 6. Las cuatro reglas que te llevás

**1. La pregunta de pertenencia.** *"¿Tiene que tickear con el campo muerto?"*
Ubica cualquier archivo, tipo o función sin discutir.

**2. El `Cargo.toml` es el lint.** No escribas reglas de arquitectura en un doc
que nadie lee: hacé que la dependencia prohibida no compile. La excepción son
los relojes — `Instant::now()` dentro de T2 no lo detecta ninguna frontera de
crate, así que se prohíbe con un tipo: `ScanInstant` no tiene constructor
público.

**3. Compilar en boot, ejecutar determinista.** Ya está en el FSM. Es el patrón
a replicar cuando crezca la lógica de escena, no a abandonar.

**4. Medí la condición, no un síntoma.** Un warning no prueba que falte un
`cfg`; un `git diff` vacío no prueba que el fixture esté versionado; un golden
verde no prueba que el orden de eventos no cambió. Cada una de esas tres pasó
en este repo.

## 7. El patrón de fallo de esta base de código

Vale la pena conocerlo porque apareció **tres veces** en cuatro sprints: **la
config declara cosas que el código no honra.**

| Dónde | Qué declaraba | Qué hacía |
|---|---|---|
| `compile_with_references(models: &impl Sized)` | validar modelos del FSM | nada — parámetro fantasma |
| `zone_vacated { min_confidence }` | umbral de confianza | nada — `Vacated` no lleva confianza |
| `features = ["rerun"]` | build opcional | nada — `src/viz` no estaba gateado |

Los tres ya están arreglados. Pero cuando agregues algo a un catálogo,
preguntate quién lo lee. **Un knob que se acepta y se ignora es peor que uno que
no existe: miente en la revisión.**

## 8. Cómo se trabaja el refactor

Ernesto implementa, Claude revisa y pule. Cinco fases por sprint —congelar,
ejecutar, verificar, revisar, cerrar— con un contrato de revisión explícito.
Está en [2-sprints.md](2-sprints.md).

La regla que ordena todo: **no se refactoriza sin red de seguridad, y no hay red
de seguridad hasta que compile.**

## Referencias

- [1-big-picture.md](1-big-picture.md) — tiers, matriz de dependencias, estado medido
- [2-sprints.md](2-sprints.md) — los sprints, sus compuertas y el método
- ADRs [027](../adrs/027-tier-architecture.md) · [028](../adrs/028-crate-boundaries.md) · [029](../adrs/029-injected-clock.md) · [030](../adrs/030-shared-mechanism-owned-vocabulary.md) · [031](../adrs/031-scene-signal-table.md)
