# Onboarding — Mana Lite

*Estado: sprints 0-4 cerrados, 2026-08-10. Si esta fecha quedó vieja, verificá
los números con los comandos de la sección 4 antes de creerles.*

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
| Tests | **365** en debug, verdes |
| `scan()` | ocho pasos nombrados |
| Funciones de producción > 80 líneas | **0** en todo el repo |
| Archivos de `src/` > 600 líneas | **0** (el mayor es `config/app.rs`, 589) |
| Reloj en T2 | `ScanInstant` solo nace de `ScanTimeline`; cero `Instant::now()` en producción |
| Vocabulario | newtypes vía `mana-id`; cero `String` en el puerto de control |
| Build | 510 crates con `default`, **264** sin `rerun` |

**Sprint 5 no está pendiente: está condicionado.** Los tres disparadores del
[ADR-031](../adrs/031-scene-signal-table.md) no se activaron —guards 18 (umbral
25), campos de contexto 7 (umbral 12), reglas por despliegue no. Son umbrales de
dolor, no metas: cuanto más alto el número, más urgente el refactor. Estar lejos
es buena noticia. Ver sección 5.

## 4. Cómo trabajar acá

### Lo primero: el repo no es autocontenido

`mana-lite` depende por path de `../inference`, que es **otro repo**
(`e7canasta/mana-inference`). Tienen que estar hermanos:

```
mana-lite-workspace/
├── mana-lite/     ← este repo
└── inference/     ← e7canasta/mana-inference
```

Sin eso, `cargo build` falla al resolver dependencias. No es un submódulo.

### Build rápido

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib"
export CARGO_TARGET_DIR=$HOME/.cache/mana-lite-target
export MANA_MODELS_HOME=/ruta/al/checkout-con-artifacts   # ver abajo
```

`CARGO_TARGET_DIR` importa: cada worktree tiene su propio `target/` y son
decenas de GB cada uno. No va en el repo porque compartir requiere ruta
absoluta, que no es portable.

`MANA_MODELS_HOME` también: los catálogos traen rutas relativas a la raíz
(`tools/model-tools/artifacts/...`) y esos pesos ONNX están gitignoreados. Un
solo test los necesita —`bootstrap_with_reader_wires_real_catalogs`, la red de
arranque— y sin la variable ese test falla y corta la suite. Todo lo demás corre
sin pesos:

```sh
cargo test --workspace --no-default-features --features ffmpeg \
  -- --skip bootstrap_with_reader_wires_real_catalogs   # 352 tests, sin ONNX
```

Eso es exactamente lo que corre el CI ([.github/workflows/gate.yml](../../.github/workflows/gate.yml)).

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

### Los umbrales del Sprint 5 son medidores de dolor, no metas

Esto se malinterpreta fácil, así que va explícito. El [ADR-031](../adrs/031-scene-signal-table.md)
dice que el refactor de la tabla de señales se hace cuando se cruce alguno de:

| Medida | Hoy | Umbral |
|---|:-:|:-:|
| Variantes de `FsmGuard` | 18 | **más de 25** |
| Campos de `FsmSceneContext` | 7 | **más de 12** |
| Reglas definidas por despliegue | no | sí |

**"18 de 25" no es una barra de progreso.** No falta ningún guard. Son 18 hoy, y
el refactor se justifica pasando 25 — porque cuantas más variantes, más caro
mantener dos enums sincronizados a mano. Estar lejos del umbral es bueno.

Los 18 de hoy: 4 de zonas (`zone_present`, `zone_occupied`, `zone_vacated`,
`all_zones_vacant`), 2 de salud de señal (`data_stale`, `data_fresh`), 1 de
profundidad (`depth_rule`), 1 de ocupación (`cardinality`), 2 de persona
(`person_present`, `person_absent`) y 8 de cara (`face_detected`, `face_absent`,
`face_in_dwell`, `face_not_in_dwell`, `face_at_edge`, `face_not_at_edge`,
`face_was_inside`, `face_was_not_inside`).

Cuando se haga, ~11 de esos 18 colapsan en una sola variante genérica
`Signal { tag, op, value }` — todos los de cara, los de persona y `cardinality`
son "leé un campo del contexto y comparalo". Los otros 7 se quedan con semántica
propia porque tienen lógica real: los de zonas necesitan el motor de zonas y sus
timers, los de salud leen `Health`, y `depth_rule` lee el snapshot de
profundidad.

El costo de hacerlo: se pierde el `match` exhaustivo del compilador sobre los
predicados genéricos. La verificación se muda a `FsmProgram::compile()`, que
valida contra los catálogos en boot. Es el modelo PLC y es aceptable — pero es
un trade, no una mejora gratis. Por eso espera al umbral.

Mientras tanto, cada booleano plano que agregues a `FsmSceneContext` es deuda
consciente que habrá que migrar.

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

## 8. Qué queda abierto

Seis sprints cerraron el objetivo original: el lazo de control es auditable por
alguien que no lo escribió. Lo que sigue abierto, medido:

| Deuda | Tamaño | Dónde duele |
|---|---|---|
| Casts numéricos sin auditar | ~412 avisos | precisión `u32→f32`, truncación `u128→u64` y `f64→u64`, pérdida de signo. Los de `mana-control` son los que importan: es el tier que decide si hay alguien en una cama |
| Comparación exacta de floats | 26 avisos | mismo argumento |
| `SceneEvent::FsmState(String)` | — | debería ser `StateId` |
| `ProgramState.models: Vec<String>` | — | debería ser `Vec<ModelId>` |
| `current_models() -> Vec<String>` | — | aloca un `Vec` **por scan**; a 5 Hz no es crítico, pero es basura evitable dentro del lazo determinista |

Ninguna necesita un sprint de diseño: son acotadas, medibles y con compuerta
mecánica obvia. El conteo global de clippy es compuerta de **no-regresión**, no
de cero.

## 9. Cómo se trabaja el refactor

Ernesto implementa, Claude revisa y pule. Cinco fases por sprint —congelar,
ejecutar, verificar, revisar, cerrar— con un contrato de revisión explícito.
Está en [2-sprints.md](2-sprints.md).

La regla que ordena todo: **no se refactoriza sin red de seguridad, y no hay red
de seguridad hasta que compile.**

## Referencias

- [1-big-picture.md](1-big-picture.md) — tiers, matriz de dependencias, estado medido
- [2-sprints.md](2-sprints.md) — los sprints, sus compuertas y el método
- ADRs [027](../adrs/027-tier-architecture.md) · [028](../adrs/028-crate-boundaries.md) · [029](../adrs/029-injected-clock.md) · [030](../adrs/030-shared-mechanism-owned-vocabulary.md) · [031](../adrs/031-scene-signal-table.md)
