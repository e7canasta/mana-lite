# MANA-LITE — Handoff

> Para que una sesión nueva retome el trabajo **sin leer nada más**.
> Lengua del proyecto: español, también en comentarios de código y commits.

*Actualizado: 2026-08-10, al cerrar el refactor por tiers.*

## 1. Qué es esto

`mana-lite` es un **PLC cuyo dispositivo de campo es una cámara**. No es una app
de visión con lógica adentro: es un controlador de cadencia fija que resulta
tener un sensor óptico.

Corre a dos tasas. El campo (RTSP → decode → ONNX) va a la velocidad que puede,
con latencia variable y fallando seguido. El programa (tracker → presencia →
ocupación → zonas → FSM → health) corre a cadencia fija y **tiene que emitir
salida en cada tick aunque el campo esté muerto**. Entre los dos hay un solo
objeto: la imagen de proceso (`ProcessImage`), congelada y fechada.

El caso clínico que corre hoy es prevención de caídas de cama:
`idle → watching → bed_approaching → bed_alert`.

**La pregunta que ubica cualquier cosa:** *si la entrada nunca vuelve a llegar,
¿esto tiene que seguir produciendo salida correcta en cada tick?* Sí → T2
programa. No → T1 campo.

## 2. Dónde estamos

**Terminado y archivado:** el refactor por tiers, seis sprints. Ver
[`docs/archive/2026-08-refactor-por-tiers/`](docs/archive/2026-08-refactor-por-tiers/README.md).

Estado medido: 6 paquetes, **366 tests**, `scan()` en ocho pasos nombrados,
cero funciones de producción sobre 80 líneas, cero archivos de `src/` sobre 600,
reloj sellado por tipo, CI corriendo la compuerta.

**Abierto:** [`docs/scene-signals/`](docs/scene-signals/README.md) — la tabla de
señales. Sin empezar. El plan son cuatro etapas en
[2-sprints.md](docs/scene-signals/2-sprints.md).

## 3. Cómo se trabaja

**Ernesto implementa con Cursor, Claude revisa etapa por etapa.** Cinco fases
por etapa: congelar, ejecutar, verificar, revisar, cerrar.

Para que la revisión sirva, cada entrega trae:

1. El **diff completo** de la etapa, no el estado final del árbol.
2. La **salida literal** de cada comando de la compuerta.
3. Las **decisiones que se desviaron del plan**, con su razón.

Orden de revisión: (1) no se rompió una frontera de tier, (2) los goldens son
idénticos o la diferencia está justificada, (3) lo puede leer alguien que no lo
escribió, (4) reuso y simplificación. En ese orden.

## 4. Arrancar

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib"
export CARGO_TARGET_DIR=$HOME/.cache/mana-lite-target
export MANA_MODELS_HOME=/ruta/al/checkout-con-artifacts
```

**El repo no es autocontenido.** Depende por path de `../inference`, que es otro
repo (`e7canasta/mana-inference`). Tienen que estar hermanos:

```
mana-lite-workspace/
├── mana-lite/
└── inference/
```

**Sin pesos ONNX** (están gitignoreados) corre todo menos un test:

```sh
cargo test --workspace --no-default-features --features ffmpeg \
  -- --skip bootstrap_with_reader_wires_real_catalogs      # 352 tests, 264 crates
```

Con `MANA_MODELS_HOME` apuntado, la suite completa: 366 debug / 365 release.

`--no-default-features` apaga `rerun` y baja el build de 510 a 264 crates. Para
iterar sobre el lazo de control, es la forma rápida.

## 5. La red de seguridad

Tres fixtures, y cuál sirve para qué **importa**:

| Fixture | Qué fija |
|---|---|
| `tests/golden/synthetic_cycle.jsonl` | ciclo de un actor → salida de log |
| `tests/golden/multi_actor_cycle.jsonl` | dos personas, cara, dos zonas → salida de log |
| `tests/golden/multi_actor_cycle.events.txt` | **la secuencia cruda de `SceneEvent` por tick** |

El tercero existe porque los JSONL **no alcanzan**: `scene_events_to_log`
descarta `SceneEvent::Occupancy` y `SceneEvent::FsmState`, así que reordenarlos
los deja verdes. Si tocás `scan()`, tu detector es `events.txt`.

Regenerar: `UPDATE_GOLDEN=1 cargo test <nombre>`. **Y después mirá el diff** —
un golden regenerado sin leerlo no es una red, es un sello de goma.

## 6. Estructura

| Tier | Qué | Dónde |
|---|---|---|
| T0 · álgebra | sin tasa, sin estado, sin reloj | `mana-id`, `mana-geometry` |
| T1 · campo | sensado y E/S; fallar es normal | `mana-media`, `mana-perception` |
| T2 · programa | cadencia fija, determinista, reloj inyectado | `mana-control` |
| T3 · reporte | JSONL, métricas, Rerun; nunca bloquea el tick | el binario (`src/`) |

**T2 no depende de T1** porque tiene que seguir corriendo cuando T1 murió. Lo
hace cumplir `core/mana-control/Cargo.toml`: tres dependencias, `mana-id`,
`mana-geometry`, `serde`. Agregar `mana-perception` ahí no rompe una convención,
rompe la compilación.

## 7. Las cuatro reglas que costaron caro

**1. Verificá que la compuerta falle cuando debe fallar.** Dos pasaron en vacío
durante sprints enteros: `git diff tests/golden/` sobre un fixture sin trackear,
y el golden JSONL como detector de orden.

**2. La config no debe declarar lo que el código no honra.** Pasó cuatro veces:
un parámetro de validación que no validaba, un `min_confidence` inevaluable, un
feature `rerun` que no apagaba nada, y `dwell = "-5s"` aceptado como 0. Un knob
que se acepta y se ignora miente en la revisión.

**3. Medí la condición, no un síntoma.** Se concluyó que faltaba un `cfg`
mirando warnings que aparecen igual con el feature encendido.

**4. Antes de diseñar una partición, mirá cuánto es test.** `logger/mod.rs` e
`infer/mod.rs` parecían god files; eran archivos normales con una montaña de
tests adentro.

## 8. Deuda registrada

| Deuda | Tamaño |
|---|---|
| Casts numéricos sin auditar en `mana-geometry` | ~202 avisos |
| Comparación exacta de floats | 26 avisos |
| `SceneEvent::FsmState(String)` | debería ser `StateId` |
| `ProgramState.models: Vec<String>` | debería ser `Vec<ModelId>` |
| `current_models() -> Vec<String>` | aloca un `Vec` por scan |

Los casts de `mana-control` ya se saldaron. El conteo de clippy es compuerta de
**no-regresión**, no de cero.

## 9. Primer paso de la próxima sesión

Leer [`docs/scene-signals/README.md`](docs/scene-signals/README.md) y arrancar
la **Etapa A**: el vocabulario, sin conectar nada. Es la única etapa sin riesgo
—nada la consume todavía— y define el contrato que las otras tres asumen.

Lo que hay que resolver ahí y no después: que `Ratio` fuera de `[0,1]` sea un
error de construcción, y que no exista forma de comparar dos `Ratio` por
igualdad. Si esas dos reglas no están en el tipo desde el principio, se cuelan
comparaciones exactas de flotante en el lazo clínico.
