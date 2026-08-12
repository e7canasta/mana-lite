# ADR-034: Muestras en slots, eventos en colas

**Status:** Accepted — implementado 2026-08-11 (`src/slot.rs`)
**Date:** 2026-08-11

## Qué cambia para el producto

Hoy el sistema **se atrasa en vez de descartar**, y en video eso es siempre la
elección equivocada.

Cuando el visor no da abasto, los frames se acumulan en cola. La cola crece, la
latencia crece con ella, y en algún momento el productor se bloquea esperando
—- que fue exactamente lo que produjo un scan de 8,2 segundos. El resultado es lo
peor de los dos mundos: se ve una imagen vieja **y** el control llegó tarde.

Lo correcto para una muestra de video es descartar la vieja y mostrar la fresca.
Nadie quiere ver el cuarto como estaba hace ocho segundos.

Después de esto:

**1. La latencia de visualización tiene cota.** Si el enlace no da, se pisan
frames y se cuenta cuántos. Lo que se ve siempre es lo último que pasó.

**2. Ningún evento clínico se pierde.** Una transición de FSM o un registro de
auditoría no es una muestra: perderlo es un bug, no una degradación. Estos
siguen su propio camino, garantizado.

**3. Los descartes se cuentan.** "Se perdieron 12 frames de visualización" es
información operativa. Perderlos en silencio es indistinguible de que el sistema
funcione bien.

## Contexto

El sistema mueve dos clases de dato que hoy se tratan igual y no lo son.

**Muestras.** Frames, detecciones, observaciones de escena. Sólo importa la más
fresca; la anterior perdió su valor en el momento en que llegó la siguiente.

**Eventos.** Transiciones de FSM, registros JSONL, cambios de estado clínico.
Cada uno importa individualmente y perder uno rompe la auditabilidad.

Hoy ambos viajan por el mismo mecanismo —- llamadas directas en el hilo compartido
— y donde hay un canal real, es una cola con contrapresión.

El caso de rerun lo ilustra sin margen de duda. Su batcher acepta
`max_bytes_in_flight` y, al llenarse, **bloquea al productor**: `re_chunk` no
ofrece ninguna política de descarte. Un frame 1080p RGB24 son 6.220.800 B; con
32 MB en vuelo, cinco frames de backlog y el pipeline queda esperando a la red.

Aplicar contrapresión a un productor de muestras es un error de categoría. La
contrapresión sirve cuando el productor **puede** ir más lento. La cámara no
puede: va a entregar el próximo keyframe llegue o no llegue el anterior.

## Decisión

**Se distinguen dos primitivas de acoplamiento y la elección entre ellas se hace
por la naturaleza del dato, no por conveniencia.**

> **Regla.** Si perder el dato viejo es *correcto*, es una muestra: va en slot.
> Si perderlo es un *bug*, es un evento: va en cola.

### `Slot<T>` — para muestras

Un elemento. `put` sobreescribe siempre y cuenta lo pisado. `take` consume.
**Nunca bloquea a ninguna de las dos puntas.** La latencia tiene cota por
construcción: como mucho, un elemento de antigüedad.

Bordes que son slots:

| Borde | Qué lleva |
|---|---|
| ingesta → percepción | `RawKeyframe` |
| percepción → control | `ProcessImage` |
| percepción → visualización | frame + detecciones |

### `Queue<T>` — para eventos

Acotada, con política declarada al construirla. En el borde control →
observabilidad la política es **no perder**: el volumen está acotado por
construcción (un puñado de eventos por scan) y la auditabilidad depende de la
completitud.

Si esa cola se llenara, es un error de diseño que hay que hacer ruidoso, no una
condición que haya que absorber en silencio.

### Instrumentación obligatoria

Cada slot publica su contador de pisadas y cada cola su ocupación máxima, en el
reporte de métricas. Un borde sin instrumentar es un borde sobre el que no se
puede razonar cuando algo va mal.

## Consecuencias

**La contrapresión de rerun deja de alcanzar al pipeline.** El slot absorbe la
diferencia de ritmo: el hilo de viz consume cuando puede y se pierde lo que no
alcanzó a mandar. Es la degradación correcta para una vista de depuración.

**Aparece una decisión explícita donde antes había una implícita.** Cada borde
nuevo obliga a contestar "¿esto es muestra o evento?". Esa pregunta es el
contenido de esta ADR: hoy no se hace, y por eso los frames viajan como si fueran
eventos.

**El `Slot<T>` es infraestructura propia.** Unas 40 líneas sobre `Mutex` +
`Condvar`. Se prefiere a traer una dependencia de canales: la semántica que hace
falta —- sobreescribir siempre, contar, nunca bloquear— es más simple que
cualquier canal genérico, y la simplicidad es el punto.

**Costo aceptado.** Un slot puede perder una muestra que hubiera sido útil si el
consumidor se demoró apenas. A las cadencias reales del sistema —- un keyframe por
segundo contra un lazo de 5 Hz— eso no ocurre salvo bajo saturación, que es
justamente cuando descartar es lo correcto.

## Cómo quedó implementado

`Slot<T>` son ~90 líneas sobre `Mutex` + `Condvar`, con tres bordes en uso:
`RawKeyframe` (ingesta → percepción), `PerceptionOutput` (percepción → control) y
`ControlDirective` (control → percepción, la realimentación).

Dos cosas que el diseño original no separaba y que el uso obligó a distinguir:

- **`take()` no bloquea nunca** y es la única forma que puede usar el lazo de
  control. Un `take` que pudiera esperar volvería a acoplar la cadencia a la
  etapa de arriba.
- **`take_blocking()`** existe para consumidores cuyo trabajo *es* la muestra
  —percepción no tiene nada que hacer sin keyframe— y que por lo tanto pueden
  dormir sin acoplar a nadie. El productor jamás espera en la condvar.

El borde de eventos es un `mpsc` y no un slot, como prescribe la regla: una
detección del JSONL perdida es un bug de auditoría. Los eventos se envían incluso
para el keyframe que entró en pánico, porque son la traza de lo que llegó a pasar
antes de romperse.

## Referencias

- [ADR-033](033-isolated-control-loop.md) — el lazo de control aislado
- [ADR-035](035-observability-port.md) — el puerto de observabilidad
- `ARCHITECTURE.md` §5.3 — presupuesto del enlace
- `workshop/scenarios/02-ingest-viz/` — evidencia medida de la saturación
