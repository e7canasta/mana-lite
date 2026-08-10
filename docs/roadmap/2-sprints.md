# Roadmap de Sprints

*Baseline original: 2026-08-09. Reformulado: 2026-08-10, con los sprints 0-3
Etapa A cerrados. Contexto: [1-big-picture.md](1-big-picture.md).*

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
| **3B** | God files del binario | pendiente | 003 |
| **4** | Consolidar `std/` y apagar `rerun` | pendiente | 028 (revisa 019) |
| **5** | Tabla de señales *(condicional)* | pendiente | 031 |

---

## Lecciones que cambian el método

Tres cosas aparecieron en los sprints 2 y 3 que no estaban en el plan original y
que conviene tener presentes en todos los que siguen.

**1. La config declara cosas que el código no honra.** Apareció tres veces:

| Dónde | Qué declaraba | Qué hacía |
|---|---|---|
| `compile_with_references(models: &impl Sized)` | validar modelos del FSM | nada — parámetro fantasma |
| `zone_vacated { min_confidence }` | umbral de confianza | nada — `Vacated` no lleva confianza |
| `features = ["rerun"]` | build opcional | nada — `src/viz` no está gateado |

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

---

## Sprints cerrados (0 · 1 · 2 · 3A)

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

**Lo que el pulido encontró después de la compuerta** (`1a098cc`, `934e663`,
`329a333`, `8e554f6`):

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

---

## Sprint 3 Etapa B — God files del binario

**Objetivo:** que los archivos de T3 se puedan leer. Es el mismo objetivo que la
Etapa A, un tier más afuera.

### Cambio respecto del plan original

`src/viz/mod.rs` (1539 líneas, el god file más grande) **sale de esta etapa** y se
hace una sola vez en el Sprint 4. Razón: el Sprint 4 ya iba a disolver `mana-viz`
adentro de `src/viz/` y borrar sus funciones muertas, y el gating de `rerun` toca
el mismo archivo. Partirlo acá significa diseñar una partición por cohesión de
1539 líneas y después invalidarla volcándole código y borrándole otro.

**No se parte código que está por morir o por mudarse.**

### Orden por cobertura, no por tamaño

La red del Sprint 1-3 cubre **T2**. Estos archivos son **T3** y su cobertura es
despareja — el orden sale de ahí, no del número de líneas:

| Orden | Archivo | Líneas | Tests directos | Red real |
|:-:|---|--:|--:|---|
| 1 | `src/logger/serialize.rs` | 830 | 0 | **byte-exacta** vía los tres goldens: es lo que los renderiza |
| 2 | `src/infer/mod.rs` | 1135 | 27 | buena |
| 3 | `src/logger/mod.rs` | 920 | 23 | buena |
| 4 | `src/config/model_loader.rs` | 719 | 3 | fina; `fsm_catalogs_compile` cubre carga de catálogos |
| 5 | `src/app/mod.rs` | 1045 | 4 | fina |
| 6 | `src/app/bootstrap.rs` | — | **0** | **ninguna** |

`bootstrap_with_reader` (469 líneas de código, la función más larga del repo) va
**última**, no primera. Es la pieza menos cubierta del repo: abrir con ella es el
movimiento más riesgoso con la red más fina. Antes de tocarla hay que construirle
red, igual que se hizo con `scan()`.

`serialize.rs` va primera por lo contrario: cero tests propios pero los goldens son
un oráculo byte-exacto de su salida. Es el refactor más seguro del repo.

### Tareas

1. Partir `logger/serialize.rs` por tipo de evento.
2. Sacar los tests de `infer/mod.rs` y `logger/mod.rs` a módulos hermanos
   (mismo movimiento que `fsm/tests/` en la Etapa A).
3. Partir `config/model_loader.rs`.
4. Partir `app/mod.rs` por responsabilidad (ciclo, observador, adaptadores).
5. Red para `bootstrap_with_reader`, y recién después partirla.

### Compuerta

```sh
cargo clippy --workspace 2>&1 | grep -c 'too many lines'   # → 0 (producción)
cargo fmt --all --check                                     # → sin salida
git diff tests/golden/                                      # → vacío
cargo test --workspace && cargo test --workspace --release
```

- [ ] Cero funciones de producción > 80 líneas en todo el repo, o excepción
      declarada **en el código** con `#[allow]` y razón
- [ ] Ningún archivo de `src/` supera las 600 líneas
- [ ] `bootstrap_with_reader` tiene test antes de que la toquen

---

## Sprint 4 — Consolidar `std/` y apagar `rerun`

**Objetivo:** de **9 paquetes a 6**, y que el feature `rerun` realmente apague
`rerun`.

### Por qué cambió el orden

El plan original abría fusionando crates. Se reordena porque medir cambió la
prioridad: **`rerun` arrastra 246 de los 510 crates del build, y su feature flag
no funciona.**

| Build | Crates |
|---|--:|
| con `default` (`ffmpeg` + `rerun`) | **510** |
| sin `rerun` | **264** |

`cargo check --workspace --no-default-features --features ffmpeg` falla con **73
errores**: 69 en `src/viz/mod.rs`, 8 en `std/mana-viz/src/lib.rs`, 3 en
`src/app/mod.rs`, 2 en `src/app/cycle.rs`, 1 en `src/viz/masks.rs`, 1 en
`src/infer/crop.rs`.

`std/mana-viz` **sí** está bien gateado (`#[cfg(feature = "rerun")]` en todo). El
que no honra el feature es `src/viz/` del binario: usa `rerun::RecordingStream` y
`rerun::blueprint::*` sin gatear, y `pub mod viz;` en `src/lib.rs` es
incondicional.

Gatearlo va primero por dos razones. La obvia: parte el tiempo de build al medio
para todos los sprints que siguen. La menos obvia: **el compilador dibuja la
frontera gratis** — lo que no compila sin el feature *es* viz; lo que compila, no
lo es. Ese es exactamente el corte que las tareas 2 y 5 necesitan, y sale de
regalo en vez de tener que adivinarlo.

### Inventario medido

```
std/mana-rtsp      65 líneas   ← un crate entero para 65 líneas
std/mana-viz      369          ← 5 de 8 funciones públicas muertas
std/mana-types    456
std/mana-video    522
std/mana-id       277          ← se queda
std/mana-geometry 2523         ← se queda
```

Uso real de los tipos `*V1` fuera de `mana-types`:

| Tipo | Usos externos | Destino |
|---|--:|---|
| `PixelFormat` | 18 | **vive** → `mana-media` |
| `RawFrameV1` | 13 | **vive** → `mana-media` |
| `SceneMsgV1` | 2 | solo `mana-viz/boxes.rs` (muerto) → borrar |
| `DetectionBatchV1` | 2 | solo `mana-viz/boxes.rs` (muerto) → borrar |
| `DetectionV1` | 0 | borrar |
| `SceneEntityV1` | 0 | borrar |
| `ZoneV1` | 0 | borrar |
| `RoiCommandV1` | 0 | borrar |

> Corrección al plan original: `PixelFormat` y `RawFrameV1` **no** son residuo de
> iceoryx2, son tipos de media vivos. Se mudan, no se borran.

Funciones públicas de `mana-viz` y su uso desde el binario:

```
boxes2d_from_xyxy      7   viva
log_frame_rgb24_owned  1   viva
log_frame_rgb24        1   viva
log_detections_2d      0   muerta
log_roi_2d             0   muerta
log_scene_event_text   0   muerta
log_static_text        0   muerta
log_zones_2d           0   muerta
```

### Tareas, en orden

1. **Gatear `rerun`.** `#[cfg(feature = "rerun")]` en `src/viz`, `src/lib.rs`, y
   los 5 sitios de llamada en `app/` e `infer/`. Cuando el feature está apagado,
   el binario corre sin visualización — no compila un stub que finge.
2. **Borrar las 5 funciones muertas de `mana-viz`.** Libera `SceneMsgV1`,
   `DetectionBatchV1` y `ZoneV1`.
3. **Borrar los `*V1` muertos** (los 6 de la tabla). Registrar en el repo de Full
   Mana OS que ahí viven.
4. **Fusionar `mana-video` + `mana-rtsp` + (`PixelFormat`, `RawFrameV1`) →
   `mana-media`.** `mana-rtsp` son 65 líneas: la frontera de crate no previene
   ninguna dependencia, solo agrega un `Cargo.toml`.
5. **Disolver `mana-viz` en `src/viz/`**, con las 3 funciones vivas.
6. Actualizar `docs/ARCHITECTURE.md` y `docs/ROADMAP.md` a la estructura de tiers.

### Compuerta

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib"

cargo metadata --no-deps --format-version 1 | jq '.packages | length'   # → 6
cargo check --workspace --no-default-features --features ffmpeg         # → 0 errores
cargo tree -p mana-lite -e normal --prefix none --no-default-features --features ffmpeg \
  | awk '{print $1}' | sort -u | wc -l                                  # → ≤ 264
grep -rn 'DetectionV1\|SceneMsgV1\|SceneEntityV1\|ZoneV1\|RoiCommandV1\|DetectionBatchV1' --include='*.rs' .   # → 0
git diff tests/golden/                                                  # → vacío
cargo test --workspace && cargo test --workspace --release
```

- [ ] El feature `rerun` apagado **compila y corre**; no es decorativo
- [ ] Cero funciones públicas sin consumidor en los crates `std/` restantes
- [ ] `mana-media` no depende de `mana-control` ni de `mana-perception`

### Trampa

La tarea 1 invita a escribir un `VizBridge` no-op para que `App` no necesite
`cfg`. **No.** Un stub que traga llamadas es otro knob que miente: la revisión no
puede distinguir "viz apagada" de "viz rota". Si el feature está apagado, las
llamadas no existen.

---

## Sprint 5 — Tabla de señales *(condicional)*

**Objetivo:** que agregar una regla de escena cueste 1-2 ediciones en vez de 6.

**No se ejecuta hasta que se dispare un umbral:**

| Disparador | Original | Hoy | Umbral |
|---|:-:|:-:|:-:|
| Variantes de `FsmGuard` | 18 | **18** | 25 |
| Campos de `FsmSceneContext` | 7 | **10** | 12 |
| Reglas definidas por despliegue | no | no | sí |

> `FsmSceneContext` pasó de 7 a 10 campos sin que nadie lo decidiera. A dos del
> umbral. Vale la pena mirarlo en la revisión del Sprint 4.

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
| Casts numéricos sin auditar | ~412 avisos | precisión `u32→f32`, truncación `u128→u64` / `f64→u64`, pérdida de signo. Concentrados en `mana-geometry` (202) y `mana-control` (77). Cada sitio necesita criterio propio: saturar, clamp, o `#[allow]` documentado |
| Comparación exacta de floats | 26 avisos | en un lazo de control, cada una merece una mirada |
| `SceneEvent::FsmState(String)` | — | debería ser `StateId` |
| `ProgramState.models: Vec<String>` | — | debería ser `Vec<ModelId>` |
| `current_models() -> Vec<String>` | — | aloca un `Vec` por scan |

El conteo global de clippy es compuerta de **no-regresión**, no de cero: hoy 753
con `too-many-lines-threshold = 80`.

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
con `cargo test`. Para no recompilar 510 crates por worktree:

```sh
export CARGO_TARGET_DIR=$HOME/.cache/mana-lite-target
```

No va en el repo: compartir requiere ruta absoluta, que no es portable entre
máquinas. `debug` y `release` son dos perfiles, o sea dos compilaciones completas
— eso es inherente a correr las dos compuertas.

---

## Referencias

- [1-big-picture.md](1-big-picture.md) — arquitectura por tiers, matriz, estado medido
- [Onboarding — Mana Lite como middleware de control](<Onboarding — Mana Lite como middleware de control.md>)
- ADRs [027](../adrs/027-tier-architecture.md) · [028](../adrs/028-crate-boundaries.md) · [029](../adrs/029-injected-clock.md) · [030](../adrs/030-shared-mechanism-owned-vocabulary.md) · [031](../adrs/031-scene-signal-table.md)
