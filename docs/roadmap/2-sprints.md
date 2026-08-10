# Roadmap de Sprints

*Baseline original: 2026-08-09. Reformulado: 2026-08-10, con los sprints 0-2, 3A
y 4 cerrados. Contexto: [1-big-picture.md](1-big-picture.md).*

Seis sprints secuenciales. El orden no es negociable: **no se refactoriza sin red
de seguridad, y no hay red de seguridad hasta que compile.**

Cada sprint tiene una **compuerta mecánica** — comandos con salida esperada, no
criterio. Un revisor externo puede verificarla sin conocer el código.

| Sprint | Objetivo | Estado | ADRs |
|---|---|---|---|
| **0** | Compilación verde | cerrado | — |
| **1** | Red de seguridad | cerrado | — |
| **2** | Sellar la frontera | cerrado | 027 · 029 · 030 |
| **3A** | Legibilidad del lazo | cerrado | 003 · 027 |
| **4** | Consolidar `std/` y apagar `rerun` | cerrado | 028 (revisa 019) |
| **3B** | God files del binario | **pendiente** | 003 |
| **5** | Tabla de señales *(condicional)* | pendiente | 031 |

> El Sprint 4 se adelantó al 3B a propósito: `src/viz/mod.rs` era el god file más
> grande **y** el objeto del Sprint 4. Partirlo antes de disolverle `mana-viz`
> adentro y gatearlo habría sido diseñar una partición para después invalidarla.

---

## Lecciones que cambian el método

Cuatro cosas aparecieron entre los sprints 2 y 4 que no estaban en el plan
original y que conviene tener presentes en los que siguen.

**1. La config declara cosas que el código no honra.** Apareció tres veces:

| Dónde | Qué declaraba | Qué hacía |
|---|---|---|
| `compile_with_references(models: &impl Sized)` | validar modelos del FSM | nada — parámetro fantasma |
| `zone_vacated { min_confidence }` | umbral de confianza | nada — `Vacated` no lleva confianza |
| `features = ["rerun"]` | build opcional | nada — `src/viz` no estaba gateado |

No es coincidencia: es el patrón de un sistema donde la config creció más rápido
que su validación. **En cada sprint, revisar si lo que se agrega a un catálogo
tiene alguien que lo lea.** Un knob que se acepta y se ignora es peor que uno que
no existe: miente en la revisión.

**2. Las compuertas pueden pasar en vacío.** `git diff tests/golden/` pasó dos
sprints enteros sin verificar nada, porque el fixture estaba atrapado por `*.jsonl`
en `.gitignore` y git no reporta archivos sin trackear. Antes de confiar en una
compuerta, **verificar que falle cuando debe fallar.**

**3. El golden JSONL no ve todo.** `scene_events_to_log` descarta
`SceneEvent::Occupancy` y `SceneEvent::FsmState`, así que reordenarlos deja el
golden verde. El detector de orden real es
`tests/golden/multi_actor_cycle.events.txt`, que fija la secuencia cruda de
`SceneEvent` por tick. Cualquier sprint que toque `scan()` se verifica contra
**ese** archivo, no contra el JSONL.

**4. Un warning no prueba ausencia de `cfg`.** Durante la revisión del Sprint 4 se
concluyó que el gating de `rerun` no estaba hecho porque `src/viz/mod.rs` seguía
tirando warnings y sus tests seguían corriendo. Ambas cosas pasan igual con el
feature **encendido**. La verificación correcta es
`cargo check --no-default-features`, no leer la salida del build por defecto.
**Medir la condición, no un síntoma que la acompaña.**

---

## Sprints cerrados

### Sprint 0 — Compilación verde · cerrado 2026-08-09

`91af4e8` cerró la migración en vuelo. El residual reconectó depth →
`ProcessImage`, desforkeó `DomStr`, y dejó las violaciones marcadas con
`FIXME(ADR-027|029)` en vez de arregladas — sin red, no se toca.

### Sprint 1 — Red de seguridad · cerrado

Golden JSONL de un ciclo completo (`synthetic_cycle.jsonl`), tests de
caracterización por componente de T2 con reloj inyectado, cadencia bajo stall, y
compilación de todos los catálogos TOML del repo.

### Sprint 2 — Sellar la frontera · cerrado 2026-08-10

`mana-id` (T0) con `DomStr` + `domain_id!`; corte `cascade → kalman/track` vía
`GateObservation`; `logger` fuera de `mana-control`; `ScanInstant` solo nace de
`ScanTimeline`; newtypes en `SceneObservation` y `Track`; shim `depth` eliminado.
`LoopId` en `ControlState` y `ScanTimeline` para la portabilidad a mana-os
multistream, con `N=1` en el binario.

**Lo que el pulido encontró después de la compuerta:**

- El corte de cascade había borrado en silencio la validación de modelos del FSM.
  Un estado que nombraba un modelo inexistente compilaba y corría **sin detector**.
- `ZoneEngine` iteraba un `HashMap`: con dos zonas cambiando en el mismo tick, el
  orden de eventos que alimenta la FSM no era reproducible. Ahora `BTreeMap`.
- La política de lints vivía bajo `[package]`, así que gobernaba solo el binario;
  los crates de tier —los que tienen la lógica determinista— eran los que se
  escapaban. Ahora `[workspace.lints]`.

### Sprint 3 Etapa A — Legibilidad del lazo · cerrado 2026-08-10

`scan()` es ocho pasos nombrados (`predict → age_input → update_presence →
update_tracking → update_occupancy → update_zones → evaluate_fsm →
evaluate_health`); tests de `fsm/` fuera de `mod.rs` repartidos por tema;
`PresenceFilter::update_at` partido; `hungarian_min` entera con
`#[allow(clippy::too_many_lines)]` — excepción declarada en el código, no solo en
el plan.

Compuerta verificada: `length>120` en `core/` → 0 · `cargo fmt --all --check` sin
salida · `too many lines` en producción de `mana-control` → 0 · goldens sin diff ·
**370 tests debug / 369 release** (la diferencia es el test `cfg(debug_assertions)`
del sello de `LoopId`).

### Sprint 4 — Consolidar `std/` y apagar `rerun` · cerrado 2026-08-10

De **9 paquetes a 6**: `mana-lite`, `mana-control`, `mana-perception`, `mana-id`,
`mana-geometry`, `mana-media`.

- `mana-media` absorbe `mana-video` + `mana-rtsp` + los tipos de frame
  (`PixelFormat`, `RawFrameV1`), que **no** eran residuo de iceoryx2 sino tipos
  vivos con 31 usos: se mudaron, no se borraron.
- `mana-viz` disuelto en `src/viz/`, gateado detrás del feature `rerun`.
- Tipos `*V1` de escena sin consumidor, borrados.
- `rerun` apagado compila y corre el lazo completo, sin stub de `VizBridge`.

**El payoff medido:** `rerun` arrastraba **246 de los 510 crates** del build y su
feature flag no funcionaba (73 errores al intentar apagarlo, 69 en
`src/viz/mod.rs`). Con el gating puesto, `--no-default-features --features ffmpeg`
compila **264**.

Compuerta: 6 paquetes · `cargo check --no-default-features --features ffmpeg` sin
errores · cero `*V1` de escena · goldens sin diff.

---

## Sprint 3 Etapa B — God files del binario *(pendiente)*

**Objetivo:** que los archivos de T3 se puedan leer. Es el mismo objetivo que la
Etapa A, un tier más afuera.

### Orden por cobertura, no por tamaño

La red de los sprints 1-3 cubre **T2**. Estos archivos son **T3** y su cobertura
es despareja — el orden sale de ahí, no del número de líneas:

| Orden | Archivo | Líneas | Tests directos | Red real |
|:-:|---|--:|--:|---|
| 1 | `src/logger/serialize.rs` | 826 | 0 | **byte-exacta** vía los tres goldens: es lo que los renderiza |
| 2 | `src/infer/mod.rs` | 1135 | 27 | buena |
| 3 | `src/logger/mod.rs` | 920 | 23 | buena |
| 4 | `src/viz/mod.rs` | 1541 | 11 | media; ahora gateado, la frontera ya está dibujada |
| 5 | `src/config/model_loader.rs` | 719 | 3 | fina; `fsm_catalogs_compile` cubre carga de catálogos |
| 6 | `src/app/mod.rs` | 1088 | 4 | fina |
| 7 | `src/app/bootstrap.rs` | 570 | **0** | **ninguna** |

`bootstrap_with_reader` (la función más larga del repo) va **última**, no primera.
Es la pieza menos cubierta: abrir con ella es el movimiento más riesgoso con la
red más fina. Antes de tocarla hay que construirle red, igual que se hizo con
`scan()`.

`serialize.rs` va primera por lo contrario: cero tests propios pero los goldens
son un oráculo byte-exacto de su salida. Es el refactor más seguro del repo.

`src/viz/mod.rs` ya no es el problema que era: el Sprint 4 le puso el `cfg`, así
que la frontera entre "esto es viz" y "esto no" está dibujada por el compilador.

### Son nueve funciones, no cinco

Medido con `too-many-lines-threshold = 80` sobre **todo** el workspace — la
compuerta de la Etapa A solo corría `-p mana-control` y por eso tres de estas
nunca aparecieron:

| Función | Líneas | Archivo |
|---|--:|---|
| `write_event` | **676** | `src/logger/serialize.rs` |
| `bootstrap_with_reader` | **490** | `src/app/bootstrap.rs` |
| *(main del probe)* | 134 | `src/bin/depth-image-probe.rs` |
| `App::run_inference` | 132 | `src/app/mod.rs` |
| `InferEngine::run` | 130 | `src/infer/mod.rs` |
| `scene_events_to_log` | 96 | `src/logger/event.rs` |
| *(helper del probe)* | 91 | `src/bin/depth-image-probe.rs` |
| `load_model_catalog` | 88 | `src/config/model_loader.rs` |
| `CascadeConfig::validate` | 85 | **`core/mana-perception/src/cascade.rs`** |

### Tareas

1. **Sacar los tests primero** — movimiento mecánico que por sí solo resuelve
   dos archivos: `logger/mod.rs` queda en 336 líneas de producción (de 920) e
   `infer/mod.rs` en 580 (de 1135). No son god files: son archivos normales con
   una montaña de tests adentro.
2. Partir `write_event` en una función por variante de `Event` (13 brazos).
3. Partir `scene_events_to_log` y lo que quede de `logger/`.
4. Partir `config/model_loader.rs`.
5. Partir `viz/mod.rs` (1182 de producción; el `cfg` del Sprint 4 ya le dibujó
   la frontera).
6. Partir `app/mod.rs` y `App::run_inference`.
7. Las tres que la compuerta del 3A no miraba: `cascade::validate` en
   `mana-perception` y las dos de `depth-image-probe`.
8. Red para `bootstrap_with_reader`, y **recién después** partirla.

Plan detallado con los cortes concretos:
`.cursor/plans/sprint_3_etapa_b_c4e17b90.plan.md`.

### Compuerta

```sh
# --workspace, no -p mana-control: el 3A no miraba perception ni el binario
cargo clippy --workspace 2>&1 | grep -c 'too many lines'   # → 0 (producción)
find src core std -name '*.rs' -exec wc -l {} + | sort -rn | head -5   # → nada > 600
cargo fmt --all --check                                     # → sin salida
git diff tests/golden/                                      # → vacío
cargo test --workspace && cargo test --workspace --release
cargo test --workspace --no-default-features --features ffmpeg
```

> Verificá que el `grep` **cuente** antes de empezar: hoy debe dar 9. Una
> compuerta que arranca en 0 porque el comando está mal escrito no detecta nada.

- [ ] Cero funciones de producción > 80 líneas en todo el repo, o excepción
      declarada **en el código** con `#[allow]` y razón
- [ ] Ningún archivo de `src/` supera las 600 líneas
- [ ] `bootstrap_with_reader` tiene test antes de que la toquen

---

## Sprint 5 — Tabla de señales *(condicional)*

**Objetivo:** que agregar una regla de escena cueste 1-2 ediciones en vez de 6.

**No se ejecuta hasta que se dispare un umbral:**

| Disparador | Hoy | Umbral |
|---|:-:|:-:|
| Variantes de `FsmGuard` | 18 | 25 |
| Campos de `FsmSceneContext` | 7 | 12 |
| Reglas definidas por despliegue | no | sí |

Mientras tanto, la decisión inmediata que cambia: **cada booleano plano que se
agregue a `FsmSceneContext` es deuda que habrá que migrar.** Agregarlo con la
forma actual sigue siendo correcto — pero conscientemente.

### Lo que no se hace nunca

Fusionar `FsmGuard` y `ProgramGuard` para ahorrar tipeo. Esa duplicación aparente
es la separación compilar-en-boot / ejecutar-determinista de un PLC. `FsmGuard`
es el texto del programa; `ProgramGuard` es el programa compilado contra los
catálogos.

Detalle completo en [ADR-031](../adrs/031-scene-signal-table.md).

---

## Deuda registrada, sin sprint asignado

Cosas medidas que no entran en ningún sprint actual. Se anotan para que la
decisión de no hacerlas sea explícita.

| Deuda | Tamaño | Nota |
|---|---|---|
| Casts numéricos sin auditar | ~412 avisos | precisión `u32→f32`, truncación `u128→u64` / `f64→u64`, pérdida de signo. Concentrados en `mana-geometry` y `mana-control`. Cada sitio necesita criterio propio: saturar, clamp, o `#[allow]` documentado |
| Comparación exacta de floats | 26 avisos | en un lazo de control, cada una merece una mirada |
| `SceneEvent::FsmState(String)` | — | debería ser `StateId` |
| `ProgramState.models: Vec<String>` | — | debería ser `Vec<ModelId>` |
| `current_models() -> Vec<String>` | — | aloca un `Vec` por scan |
| Helpers muertos en `logger/serialize.rs` | 5 fns | `write_bool`, `write_optional_*`; se resuelven al partir el archivo en 3B |

El conteo global de clippy es compuerta de **no-regresión**, no de cero.

---

## Fases del sprint

Ernesto implementa, Claude revisa y pule. Cada sprint corre el mismo ciclo de
cinco fases.

| # | Fase | Qué | Quién |
|---|---|---|---|
| 1 | **Congelar** | Antes de tocar nada: correr la suite, guardar los goldens, anotar qué no puede cambiar. Si el sprint no puede nombrar su invariante, no está listo para empezar. | Ernesto |
| 2 | **Ejecutar** | Implementación, commits chicos y temáticos. Un commit no mezcla *mover* código con *cambiar* código — esa mezcla es lo que hace irrevisable un refactor. | Ernesto |
| 3 | **Verificar** | Correr la compuerta del sprint. Es mecánica: comandos con salida esperada. Si una falla, el sprint no está listo para revisión. | Ernesto |
| 4 | **Revisar** | Paso de revisión sobre el diff completo: fronteras, invariantes de tier, comportamiento preservado, legibilidad para un tercero. | Claude |
| 5 | **Cerrar** | Actualizar el status del ADR, anotar lo aprendido que contradiga el plan, ajustar el sprint siguiente. | Ambos |

### Contrato de revisión

Para que la fase 4 sea útil y no una lectura genérica, cada entrega trae tres
cosas:

1. El **diff completo** del sprint, no el estado final del árbol.
2. La **salida literal** de cada comando de la compuerta.
3. Las **decisiones que se desviaron del plan**, con su razón. Una desviación
   justificada corrige el roadmap; una desviación silenciosa lo invalida.

### Orden de revisión

1. La **frontera de tier** no se rompió.
2. Los **goldens** son idénticos, o la diferencia está justificada.
3. El código nuevo lo puede **leer alguien que no lo escribió**.
4. **Reuso y simplificación.**

En ese orden: un cleanup elegante que rompe una frontera se rechaza antes de
mirarle el estilo.

### Antes de confiar en una compuerta

Verificar que **falle cuando debe fallar**. Dos de las compuertas de este roadmap
pasaron en vacío durante sprints enteros: `git diff tests/golden/` sobre un
fixture sin trackear, y el golden JSONL como detector de orden de eventos.

---

## Nota operativa: tiempo de build

Cada worktree tiene su propio `target/`, y `cargo clippy` no comparte artefactos
con `cargo test`. Para no recompilar el workspace por worktree:

```sh
export CARGO_TARGET_DIR=$HOME/.cache/mana-lite-target
```

No va en el repo: compartir requiere ruta absoluta, que no es portable entre
máquinas. `debug` y `release` son dos perfiles, o sea dos compilaciones completas
— eso es inherente a correr las dos compuertas.

Desde el Sprint 4, `--no-default-features --features ffmpeg` compila 264 crates en
vez de 510. Para iterar sobre T2, es la forma rápida.

---

## Referencias

- [1-big-picture.md](1-big-picture.md) — arquitectura por tiers, matriz, estado medido
- [0-onboarding.md](0-onboarding.md) — entrada para alguien nuevo: modelo mental, tiers, cómo trabajar
- ADRs [027](../adrs/027-tier-architecture.md) · [028](../adrs/028-crate-boundaries.md) · [029](../adrs/029-injected-clock.md) · [030](../adrs/030-shared-mechanism-owned-vocabulary.md) · [031](../adrs/031-scene-signal-table.md)
