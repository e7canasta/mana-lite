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

**Terminado:** el lazo de control corre aislado. Tres etapas con dueños de
ejecución distintos —ingesta, percepción, control— unidas por bordes que no
bloquean. **El sistema cumple su invariante por primera vez**: mantiene cadencia
aunque la inferencia tarde más de un periodo, aunque el visor sature el enlace o
aunque percepción entre en pánico.

Verificado en campo, no por argumento:

| | antes | después |
|---|---|---|
| atraso del lazo, p95 | 101–154 ms | **1,4–3,3 ms** |
| con el visor saturado | 41 s de bloqueo, 147 reconexiones | **0 y 0** |
| latencia de inferencia | 194–217 ms | 194–217 ms (igual) |

No se optimizó nada: la inferencia dejó de cobrárselo al lazo. Registro completo
en [`docs/archive/2026-08-lazo-aislado/`](docs/archive/2026-08-lazo-aislado/README.md);
decisiones en `docs/adrs/033-035`; cómo está construido hoy, en
[ARCHITECTURE.md](ARCHITECTURE.md).

Antes de eso: el refactor por tiers, seis sprints (ADR-027, ADR-028), y la tabla
de señales como contrato ([ADR-032](docs/adrs/032-scene-signals-as-contract.md)).

**Abierto:** nada de arquitectura de ejecución. Lo que queda está en §8 y §9.

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

## 7. Las cinco reglas que costaron caro

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

**5. El instrumento no es la medición.** Una compuerta o una línea de reporte
puede existir, verse correcta y no medir lo que dice. Pasó cuatro veces en un
solo día:

- `seen` enmascarado con `processed`: cada straddle de ventana sumaba deriva
  permanente y el par dejaba de conservarse.
- La tolerancia de incumplimiento puesta **por debajo** del piso del
  temporizador: 96% de vencimientos incumplidos en el escenario *sano*, y el
  contador no distinguía un lazo perfecto de uno parado 188 ms.
- Cuatro ceros al lado de `0 scans con evidencia`, que se leían como "la
  evidencia tiene 0 ms de edad" — lo contrario de lo que pasaba.
- La edad de la evidencia viajando dentro de un evento apagado por defecto: la
  única magnitud con consecuencia clínica era la única que no llegaba al JSONL.

Las cuatro las encontró una corrida; ninguna, una revisión de código. Antes de
creerle a un número, verificá **qué lo alimenta, contra qué umbral se compara, y
por dónde sale**. Es la regla 2 llevada a la instrumentación: un knob que se
ignora miente en la revisión, y un instrumento mal calibrado miente en la
autopsia, que es peor.

## 8. Deuda registrada

Ninguna bloquea nada. En orden de lo que más molesta al leer el repo.

| Deuda | Qué es | Cómo se salda |
|---|---|---|
| Wiki generada desactualizada | 21 archivos contra el commit `ad24740d`, describen el super loop y tipos borrados | **regenerar**, no editar a mano |
| Dos documentos de arquitectura | `ARCHITECTURE.md` (ejecución) y `docs/ARCHITECTURE.md` (workspace por crates) | decidir si se funden |
| Casts numéricos sin auditar en `mana-geometry` | ~202 avisos de clippy | compuerta de no-regresión, no de cero |
| Comparación exacta de floats | 26 avisos | ídem |
| `SceneEvent::FsmState(String)` | debería ser `StateId` | tipado del vocabulario |
| `ProgramState.models: Vec<String>` | debería ser `Vec<ModelId>` | ídem |
| `current_models() -> Vec<String>` | aloca un `Vec` por scan | irrelevante a 5 Hz |
| Etapas sin supervisor | informan que murieron (`stage_died`) y nadie las reinicia | superficie real chica: los pánicos de percepción se atrapan y retina reconecta sola |

## 9. Lo que sigue, y no es arquitectura

**La pregunta abierta es de umbrales clínicos, no de código.** El sistema mide
por primera vez la edad de la evidencia sobre la que decide:

```
no puede decidir sobre nada más fresco que   335 ms
peor caso normal                            1 140 ms
stale_warn_ms                               5 000 ms   ← 4,4×
data_stale_ms                              10 000 ms   ← 8,8×
```

Los 335 ms de piso no son un defecto: son lo que cuesta producir la evidencia
(decode + inferencia) más la fase con la que el keyframe cae en la grilla de
scan, que la fija el GOP de la cámara y no nosotros.

Lo que hay que decidir es el resto: **entre "esto ya no es normal" y "dejo de
confiar en lo que veo" hay casi nueve segundos**, y en ese intervalo el sistema
sigue decidiendo con los timers clínicos corriendo sobre evidencia congelada
(`single_confirm_ms = 3000`, `empty_confirm_ms = 8000`). Una persona se levanta
de la cama y llega al piso en un par de segundos.

Puede estar bien —un umbral corto genera ceguera espuria con cualquier hipo de
red— pero hoy nadie tomó esa decisión mirando este número, porque el número no
existía. Es la conversación que abre el trabajo de producto.

**Producto, sin planificar.** El catálogo de señales ya es un contrato
([ADR-032](docs/adrs/032-scene-signals-as-contract.md)): cambiar cuándo suena una
alerta es editar un TOML. Si eso se sostiene, el cuello de botella dejó de ser
la ingeniería y pasó a ser saber qué escenario clínico sigue.
