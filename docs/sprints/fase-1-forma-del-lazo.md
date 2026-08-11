# Fase 1 — La forma del lazo

Plan de implementación. Requiere la [Fase 0](fase-0-sanear-base.md) cerrada.

**Referencias:** [ROADMAP.md](../../ROADMAP.md) · [ARCHITECTURE.md](../../ARCHITECTURE.md) §1-2 ·
[ADR-033](../adrs/033-isolated-control-loop.md)

---

## Objetivo

Que el lazo de control **mida y declare su propio incumplimiento de cadencia**.

Esta fase no mejora la cadencia. No mueve nada de hilo. Lo único que produce es
un número: cuánto llega tarde el scan, hoy, en producción. Ese número es el que
justifica las fases 2 a 5 —- y sin él, esas fases son una apuesta.

`HANDOFF.md` define el sistema como *"un PLC cuyo dispositivo de campo es una
cámara"*, y su invariante como *"tiene que emitir salida en cada tick aunque el
campo esté muerto"*. Hoy no hay forma de saber si eso se cumple. Esta fase la
crea.

**Criterio de salida.** El reporte de ciclo publica el atraso real del scan, y
ese atraso es distinto de cero cuando la inferencia está prendida.

---

## Cambio respecto del roadmap original

El roadmap decía "introducir `Slot<T>` y el lazo con deadline". **`Slot<T>` se
mueve a la Fase 2.**

Motivo: en la Fase 1 no tendría consumidor. Construir una primitiva de
sincronización antes de que exista algo que la use es diseñar contra un usuario
imaginario —- y la primera forma que se elige sin presión de un caso real suele
ser la equivocada. En la Fase 2 el relay de visualización la necesita de verdad,
y la va a moldear con requisitos concretos.

Esta fase queda entonces más chica de lo planeado, y eso está bien: es la que
menos riesgo tiene y la que produce el instrumento de las demás.

---

## El problema que se resuelve

`src/app/mod.rs:71` construye el reloj del scan así:

```rust
let mut scan_interval = tokio::time::interval(scan_config.period());
```

Sin `set_missed_tick_behavior`, o sea `MissedTickBehavior::Burst`. Cuando el
procesamiento de un keyframe bloquea la task 216 ms, el tick no puede dispararse;
después salen los perdidos uno tras otro para alcanzar el reloj.

El resultado es que **el atraso se absorbe y desaparece de la medición**. En
cualquier reporte de ciclo:

```
cycle: 5.2 Hz — 26 scans in 5s | p95 200ms max 200ms min 1ms | 0 overruns
                                                     ^^^^^^^
```

Ese `min 1ms` es el tick recuperado. La p95 y el max se ven perfectos porque el
periodo *promedio* se mantiene: Burst compensa. Lo que no se ve es que hubo un
scan que debió correr y no corrió a tiempo.

La distinción importa y no es sutil:

- **Periodo de ciclo** (lo que ya se mide): cuánto pasó entre dos scans.
  Con Burst se autocorrige, así que se ve sano.
- **Atraso de deadline** (lo que falta): cuánto después de su vencimiento
  arrancó cada scan. Eso *no* se autocorrige y es lo que un PLC llama
  incumplimiento.

---

## T1 — Lazo con deadline explícito

### Qué

Reemplazar `interval.tick()` por un vencimiento calculado, y medir el sobrepaso.

```rust
let period = scan_config.period();
let mut next_deadline = Instant::now() + period;

loop {
    tokio::select! {
        kf = self.ingest.poll_freshest_keyframe() => { /* igual que hoy */ }
        () = tokio::time::sleep_until(next_deadline.into()) => {
            let now = Instant::now();
            // Cuánto después de su vencimiento arranca este scan. Con la task
            // bloqueada por decode/inferencia, esto es > 0 y hoy no se ve.
            let late = now.saturating_duration_since(next_deadline);
            self.metrics.tick_scan_deadline(late);

            // El próximo vencimiento se calcula desde el anterior, no desde
            // `now`: así el reloj no deriva y los vencimientos perdidos se
            // recuperan de a uno, igual que `Burst`. De esa recuperación
            // depende que `ScanTimeline` siga alineado con el reloj de pared
            // (ARCHITECTURE.md §2.3).
            next_deadline += period;

            self.scan_tick(config, now);
        }
    }
}
```

### La sutileza que no se puede perder

`next_deadline += period` **debe** ser relativo al vencimiento anterior, nunca
`Instant::now() + period`. Si se calcula desde `now`, el reloj deriva hacia
adelante cada vez que hay atraso, y como `ScanTimeline` cuenta ticks
(`ARCHITECTURE.md` §2.2), **todos los tiempos clínicos configurados empezarían a
durar más de lo que dicen**, sin error visible.

Es la misma trampa que documenta el invariante crítico de §2.3, sólo que
expresada en la forma nueva. Ponerlo en un comentario en el código, no sólo acá.

### Qué hay que verificar

Que la cadencia observada no cambie. Esta fase no debe mejorar ni empeorar nada:
si el `cycle:` promedio se mueve, la reescritura introdujo un cambio de
comportamiento que no estaba pedido.

---

## T2 — Publicar el atraso

### Métricas

Sumar a `MetricsEngine` un acumulador de atraso, con la misma forma que ya tienen
los demás: min / p95 / max sobre la ventana, más un contador de vencimientos
incumplidos.

Un **incumplimiento** es `late > 0`. No hace falta umbral: el vencimiento o se
cumple o no. El umbral que ya existe (`cycle_budget_ms`) sigue midiendo otra
cosa —- el periodo— y las dos conviven porque responden preguntas distintas.

Línea de reporte sugerida, junto a la de ciclo:

```
scan:  26 deadlines in 5s | late p95 0ms max 231ms | 5 missed
```

### JSONL

El atraso es una observación de salud, no sólo una métrica de consola. Debe
salir en el JSONL con el resto de la salud, para que una revisión de incidente
pueda responder *"¿el lazo estaba corriendo a tiempo cuando pasó esto?"*.

Seguir el patrón de los eventos `health` existentes.

### Lo que NO hay que hacer todavía

**No** hacer que el atraso dispare degradación ni alimente el FSM. Eso es Fase 5,
y depende de que un atraso signifique algo preciso —- hoy, con todo compartiendo
hilo, no dice de quién es la culpa.

---

## T3 — Escenario 03: la línea base con inferencia

### Qué

Un escenario nuevo, `workshop/scenarios/03-ingest-infer/`, primero con
`pipeline.infer = true` y todo lo demás apagado.

Es el primer escenario con inferencia encendida, y su propósito en esta fase es
**medir**, no aprobar. La compuerta no es "el atraso es chico": es "el atraso es
visible y consistente con el costo de inferencia medido".

### Criterios

|Criterio|Qué se espera|
|---|---|
|Atraso visible|`late max` del orden de la latencia de inferencia (~216 ms), no 0|
|Frecuencia|Aproximadamente un incumplimiento por keyframe procesado|
|Cadencia media|El `cycle:` promedio sigue en ~200 ms: Burst compensa, y eso está bien |
|Ingesta|Sin regresión respecto del escenario 01|

Si el atraso diera cero con inferencia prendida, **la medición está mal**, no el
sistema: sabemos que la task se bloquea 216 ms.

### Por qué este escenario es el pivote del roadmap

El número que salga de acá es la justificación cuantificada de la Fase 3. Hasta
hoy el argumento para sacar la inferencia del lazo es teórico —- "216 ms bloquean
un ciclo de 200 ms". Después de esta fase es un número medido en la instalación
real, con esta cámara y este modelo.

---

## Compuertas de la fase

```sh
cargo test --release
cargo run --release -- --config workshop/scenarios/01-ingest-only/mana.toml     # 180s
cargo run --release -- --config workshop/scenarios/03-ingest-infer/mana.toml    # 180s
./workshop/scenarios/02-ingest-viz/run-variant.sh b-jpeg-native 180
```

|Compuerta|Criterio|
|---|---|
|Suite|verde|
|Escenario 01|sin regresión: el atraso debe ser ~0 sin inferencia|
|Escenario 03|atraso visible y del orden de la latencia de inferencia|
|Escenario 02|sin regresión respecto de la Fase 0|
|No-regresión|el `cycle:` promedio no se movió en ningún escenario|

El escenario 01 es el control del experimento: sin inferencia no hay nada que
bloquee el lazo, así que el atraso tiene que dar prácticamente cero. Si diera
alto, el instrumento está midiendo mal.

---

## Riesgos

**Bajo, con una excepción.** El cambio es local a `App::run` y no toca dominio ni
configuración.

La excepción es el cálculo del vencimiento. Un `next_deadline = now + period` en
vez de `+= period` no rompe ningún test, no produce ningún error, y hace que
todos los tiempos clínicos se estiren en silencio. **Es el único punto de esta
fase que puede causar daño clínico, y no tiene test que lo atrape** salvo que se
escriba uno explícito.

Escribirlo: un test que avance un reloj falso con atrasos y verifique que la
suma de periodos no deriva.

---

## Qué se revisa al cierre

1. Que `next_deadline` sea relativo al vencimiento anterior, con comentario que
   explique por qué, y con test.
2. Que el atraso y el periodo se reporten como cosas distintas, sin mezclarlos.
3. Que el escenario 03 registre sus números en el README, no un "dio bien".
4. Que el escenario 01 muestre atraso ~0 — si no, el instrumento miente.
5. Que nada de esta fase intente degradar o reaccionar al atraso.
