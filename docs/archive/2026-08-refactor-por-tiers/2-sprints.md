# Roadmap de Sprints

*Baseline: 2026-08-09. Contexto: [1-big-picture.md](1-big-picture.md).*

Seis sprints secuenciales. El orden no es negociable: **no se refactoriza sin red
de seguridad, y no hay red de seguridad hasta que compile.**

Cada sprint tiene una **compuerta mecánica** — comandos con salida esperada, no
criterio. Un revisor externo puede verificarla sin conocer el código.

| Sprint | Objetivo | ADRs |
|---|---|---|
| **0** | Compilación verde | — |
| **1** | Red de seguridad | — |
| **2** | Sellar la frontera | 027 · 029 · 030 |
| **3** | Legibilidad del lazo | 003 · 027 |
| **4** | Consolidar `std/` | 028 (revisa 019) |
| **5** | Tabla de señales *(condicional)* | 031 |

---

## Sprint 0 — Compilación verde

**Objetivo:** cerrar la migración en vuelo sin tomar una sola decisión de diseño.
Es el único sprint donde la regla es *no pensar*: arreglar imports, nada más.

**Status (2026-08-09): cerrado.** `91af4e8` cerró la migración (compilación
verde, suite verde). El residual reconectó depth → `ProcessImage`, desforkeó
`DomStr`, y marcó las violaciones restantes con `FIXME(ADR-027|029)`.

### Tareas

1. ~~Reescribir `src/lib.rs`: eliminar los 13 `pub mod` de archivos borrados.~~
2. ~~Resolver `crate::config` — mover a `mana-control::config` las structs de política.~~
3. ~~Resolver `crate::depth` usando el shim `pub mod depth` (conservado).~~
4. ~~Restaurar `mana-control/src/domain.rs` con `DomStr` completo — copia fiel.~~ Residual: mecanismo en `mana-control`, `ModelId`/`ClassName` en el binario.
5. ~~Arreglar los imports de los tests a las rutas nuevas.~~
6. Dejar las violaciones marcadas: `#[path]` de cascade → `FIXME(ADR-027)`;
   `ScanInstant::now()` / `Health::{new,touch,evaluate}` → `FIXME(ADR-029)`.
7. **Residual:** reconectar `evaluate_depth_rules` → `set_depth` (había quedado
   en `reset_depth`); test de caracterización `depth_evidence_reaches_fsm`.

### Compuerta

```sh
cargo check --workspace --all-targets     # → 0 errores
cargo test --workspace                    # → verde (incluye depth_evidence_reaches_fsm)
git diff tests/golden/synthetic_cycle.jsonl   # → vacío
```

- [x] Cero tipos nuevos de diseño, cero traits nuevos; `project_depth_results` es
      mapeo 1:1 entre forks temporales, no un tipo nuevo

### Trampa

Este sprint invita a *"ya que estoy, arreglo esto otro"*. **No.** Cualquier
mejora que se cuele acá viaja sin red de seguridad. Las violaciones de frontera
se dejan marcadas, no arregladas — excepto el cable de depth, que era
comportamiento clínico perdido, no una mejora.

---

## Sprint 1 — Red de seguridad

**Objetivo:** 5 tests no alcanzan para refactorizar un lazo de control clínico.
Antes de mover una línea de diseño, congelar el comportamiento observable.

### Tareas

1. Congelar el contrato JSONL: golden de un ciclo completo con transiciones de
   ocupación, entrada/salida de zona y al menos un ciclo
   `blind → data_fresh → recuperado`.
2. Tests de caracterización por componente de T2, con reloj inyectado:
   histéresis de presencia, cada transición de `OccupancyStateMachine`, guards de
   dwell del FSM, reglas de profundidad.
3. Test de cadencia bajo stall: N ticks sin frame nuevo deben producir N scans y
   la secuencia de health esperada.
4. Test que compile **todos** los catálogos TOML del repo con
   `FsmProgram::compile()` y espere cero errores.
5. **Depth end-to-end vía `App`:** el residual del Sprint 0 cubrió el contrato
   de control (`ProcessImage` → `scan` → guard) y el mapeo
   `project_depth_results`. Falta un test que pase por
   `App::evaluate_depth_rules` con un `DepthFrame` sintético y afirme que
   `control_image.depth_snapshot().is_triggered(...)` es `Some(true)`. Es la
   clase de agujero que dejó pasar la regresión `reset_depth`.

### Compuerta

- [ ] Cada componente de T2 tiene **≥ 1 test de transición y ≥ 1 de no-transición**
- [ ] El golden JSONL cubre **ocupación, zona, FSM y health** en una corrida
- [ ] Todo test de T2 construye su propia `ScanTimeline` — cero `Instant::now()` en tests nuevos
- [ ] Los tests pasan con `--release` y con `-- --test-threads=1` por igual
- [ ] Existe un test que ejercita depth **a través del adaptador de `App`**, no solo del port de control

### Por qué es un sprint propio

La suite ya pasa, pero cubría 5 casos de integración y ninguno tocaba depth
end-to-end vía `App`. El residual del Sprint 0 lo demostró: `scan()` funcionaba
con evidencia, y el adaptador la tiraba. No se puede verificar "comportamiento
preservado" en los sprints 2-4 sin congelar primero esos caminos.

---

## Sprint 2 — Sellar la frontera

**Objetivo:** hacer que las violaciones de tier sean imposibles de compilar.
Preservando comportamiento — la red del Sprint 1 lo verifica.

### Tareas

1. Crear `mana-id` (T0) con `DomStr` + `domain_id!`. Cada crate declara sus
   propias instancias ([ADR-030](../adrs/030-shared-mechanism-owned-vocabulary.md)).
2. Cortar `cascade → kalman/track`: percepción no lee estado del programa. Es
   realimentar la salida al sensor sin pasar por la imagen de proceso.
3. Sacar `logger::Event` de `scan.rs`. `scan()` ya devuelve `Vec<SceneEvent>`;
   el mapeo a JSONL se muda al binario (T3).
4. Eliminar `ScanInstant::now()` y todos los wrappers sin `_at`. `ScanInstant`
   solo nace de `ScanTimeline` ([ADR-029](../adrs/029-injected-clock.md)).
5. Migrar `SceneObservation.class` y `source_models` de `String` a newtypes.
6. Eliminar el shim `pub mod depth`: medición → `mana-perception`, reglas
   clínicas → `mana-control`.

### Compuerta

```sh
grep -rn 'kalman\|track' core/mana-perception/src            # → 0
grep -rn 'logger' core/mana-control/src                      # → 0
grep -rn 'Instant::now' core/mana-control/src | grep -v 'cfg(test)'   # → 0
git diff tests/golden/                                       # → vacío
```

- [ ] `mana-control/Cargo.toml` = `mana-id` + `mana-geometry` + `serde`. Nada más.

### Decisión a tomar acá

**Multi-cámara.** Si está en el roadmap a 12 meses, parametrizar `ControlState` y
`ScanTimeline` por lazo es barato ahora y caro después de 20 reglas más. Es el
único riesgo que ningún sprint cubre por defecto.

---

## Sprint 3 — Legibilidad del lazo

**Objetivo:** que un revisor externo pueda auditar el lazo de control. Empieza
por `scan.rs`, **no** por `viz/mod.rs`.

**Status (2026-08-10): Etapas A y B cerradas.** `scan()` es ocho pasos
nombrados; tests de `fsm/` fuera de `mod.rs`; god files del binario partidos;
`bootstrap_with_reader` tenía red **antes** de partirse. Detector de orden:
`tests/golden/multi_actor_cycle.events.txt` (el JSONL no ve
`Occupancy`/`FsmState` — `scene_events_to_log` los descarta de forma explícita).

**Lección Etapa B:** `logger/mod.rs` e `infer/mod.rs` se resolvieron solo
moviendo tests a módulos hermanos — no eran god files de producción. La próxima
vez que un archivo parezca un god file, mirar primero cuánto de eso son tests.

### Tareas

1. ~~`rustfmt` sobre el workspace~~ (Etapa A)
2. ~~Partir `scan()` en los ocho pasos del ciclo~~ (Etapa A)

   ```
   predict → age_input → update_presence → update_tracking
           → update_occupancy → update_zones → evaluate_fsm → evaluate_health
   ```

   No es descomposición estética: es el ciclo de scan hecho explícito.
3. ~~Partir los god files del binario (Etapa B):~~ serialize por variante,
   `scene_events_to_log`, `model_loader`, `viz/`, `app`/`run_inference`,
   gate-misses (`CascadeConfig::validate`, `depth-image-probe`,
   `InferEngine::run`), red + split de `bootstrap_with_reader`.
4. ~~Mover tests fuera de `fsm/mod.rs`~~ (Etapa A). ~~`logger/` e `infer/`~~
   (Etapa B — con eso solos bajaron bajo 600).

### Compuerta (Etapa B — permanente `--workspace`)

```sh
cargo clippy --workspace 2>&1 | grep -c 'too many lines'          # → 0
find src -name '*.rs' -exec wc -l {} + | sort -rn | head -5       # → nada > 600
cargo fmt --all --check                                            # → sin salida
git diff tests/golden/                                             # → vacío
cargo test --workspace
cargo test --workspace --release
cargo test --workspace --no-default-features --features ffmpeg
```

- [x] Cero funciones de producción > **80 líneas** en todo el repo
      (`hungarian_min` exceptuada en código)
- [x] Ningún archivo de `src/` supera las 600 líneas
- [x] `bootstrap_with_reader` tenía test **antes** de que la tocaran
- [x] Los tres goldens byte-idénticos
- [x] `scan()` lee como **ocho llamadas nombradas** en el orden del ciclo

### Por qué este orden

El archivo con más líneas de producción (`viz/mod.rs`) no era el más urgente en
A — el ilegible del lazo sí. En B, `serialize`/`write_event` primero (mejor red:
goldens) y `bootstrap_with_reader` al final (peor cobertura).

---

## Sprint 4 — Consolidar `std/` y apagar `rerun`

**Objetivo:** de 9 paquetes a 6, y que el feature `rerun` apague de verdad la
visualización.

**Status (2026-08-10): cerrado.** `mana-media` absorbe video/rtsp/frame types;
`mana-viz` disuelto en `src/viz/`; tipos iceoryx de escena borrados; build sin
`rerun` compila el lazo completo.

### Tareas

1. ~~Gatear `rerun` en el binario (sin stub de VizBridge).~~
2. ~~Borrar helpers muertos de mana-viz; borrar `*V1` de escena sin consumidor.~~
3. ~~Fusionar `mana-video` + `mana-rtsp` + (`PixelFormat`, `RawFrameV1`) en
   `mana-media`.~~
4. ~~Disolver `mana-viz` en `src/viz/`.~~
5. ~~Actualizar `docs/ARCHITECTURE.md` y `docs/ROADMAP.md`.~~

### Compuerta

```sh
cargo metadata --no-deps --format-version 1 | jq '.packages | length'   # → 6
cargo check --workspace --no-default-features --features ffmpeg         # → 0 errores
grep -rn 'DetectionV1\|SceneMsgV1\|SceneEntityV1\|ZoneV1\|RoiCommandV1\|DetectionBatchV1' \
  --include='*.rs' .                                                    # → 0
git diff tests/golden/                                                  # → vacío
```

- [x] Feature `rerun` apagado compila y corre sin visualización
- [x] `mana-media` no depende de control ni perception
- [x] Seis paquetes en el workspace

---

## Sprint 5 — Tabla de señales *(condicional)*

**Objetivo:** que agregar una regla de escena cueste 1-2 ediciones en vez de 6.

**No se ejecuta hasta que se dispare un umbral:**

| Disparador | Hoy | Umbral |
|---|:-:|:-:|
| Variantes de `FsmGuard` | 18 | 25 |
| Campos de `FsmSceneContext` | **7** | 12 |
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

## Fases del sprint

Ernesto implementa, Claude revisa y pule. Cada sprint corre el mismo ciclo de
cinco fases.

| # | Fase | Qué | Quién |
|---|---|---|---|
| 1 | **Congelar** | Antes de tocar nada: correr la suite, guardar el golden, anotar qué no puede cambiar. Si el sprint no puede nombrar su invariante, no está listo para empezar. | Ernesto |
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
2. El **golden** es idéntico, o la diferencia está justificada.
3. El código nuevo lo puede **leer alguien que no lo escribió**.
4. **Reuso y simplificación.**

En ese orden: un cleanup elegante que rompe una frontera se rechaza antes de
mirarle el estilo.

---

## Referencias

- [1-big-picture.md](1-big-picture.md) — arquitectura por tiers, matriz, estado medido
- [0-onboarding.md](0-onboarding.md) — entrada para alguien nuevo: modelo mental, tiers, cómo trabajar y qué queda abierto
- ADRs [027](../adrs/027-tier-architecture.md) · [028](../adrs/028-crate-boundaries.md) · [029](../adrs/029-injected-clock.md) · [030](../adrs/030-shared-mechanism-owned-vocabulary.md) · [031](../adrs/031-scene-signal-table.md)
