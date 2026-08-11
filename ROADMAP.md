# Roadmap — Aislamiento del lazo de control

Plan de ejecución para [ADR-033](docs/adrs/033-isolated-control-loop.md),
[ADR-034](docs/adrs/034-slots-and-queues.md) y
[ADR-035](docs/adrs/035-observability-port.md).

Objetivo: que la cadencia del lazo de control sea una **garantía verificable** y
no una aspiración. Estado actual y evidencia en
[ARCHITECTURE.md](ARCHITECTURE.md).

---

## Reglas del plan

**Cada fase entrega valor sola.** Si el plan se detiene después de cualquier
fase, lo entregado sigue siendo una mejora y el sistema queda consistente. No hay
fases que sólo tengan sentido si viene la siguiente.

**Cada fase tiene una compuerta medible.** Un escenario del `workshop/` con
criterio escrito antes de correr. No se avanza con la anterior en rojo.

**Ninguna fase cambia el dominio.** `ProcessImage`, `SceneEvent`, el catálogo de
señales, la política clínica y la configuración quedan intactos. Si una fase
necesita tocar una regla clínica, la fase está mal planteada.

**La base tiene que estar verde antes de empezar.** No se puede refactorizar
contra una línea base en rojo: con bugs conocidos en la base, es imposible
distinguir "lo rompió el refactor" de "ya estaba roto". De ahí la Fase 0.

---

## Fase 0 — Sanear la línea base

**Por qué ahora.** Las compuertas del refactor son los escenarios del workshop.
Sirven sólo si el punto de partida es verde.

| # | Trabajo | Motivo |
|---|---|---|
| 0.1 | Commitear lo ya corregido: cancelación de ingesta, liveness de viz, contador de overruns, encoding JPEG | No se arranca un refactor sobre trabajo sin commitear |
| 0.2 | Borrar `[viz] image_max_res` | Knob que desalinea overlays a cambio de un beneficio que JPEG ya da mejor; no se arrastra a la arquitectura nueva |
| 0.3 | Ventana de vencimiento al dedupe de keyframes | Bug de corrección clínica, independiente del refactor, y puede poner el escenario 01 en rojo espuriamente |

**Explícitamente fuera de esta fase**, para no hacer el trabajo dos veces:

- *Escalamiento del presupuesto de ciclo.* La política de qué degradar cuando el
  lazo llega tarde depende de la arquitectura destino. Va en la Fase 5.
- *Carga de modelos con `pipeline.infer = false`.* La Fase 3 toca esa zona.

**Compuerta.** `01-ingest-only` y `02-ingest-viz` en verde, corridas largas, con
la suite completa pasando.

---

## Fase 1 — La forma del lazo

**Qué.** Introducir `Slot<T>` y reescribir el lazo de control como un ciclo con
deadline propio: calcular el próximo vencimiento, dormir hasta él, correr, medir
el sobrepaso. **Todavía en un solo hilo.**

**Por qué primero.** Fija la forma sin mover nada. Un lazo con deadline explícito
en un hilo compartido no mejora la cadencia —- las otras etapas lo siguen
bloqueando— pero **hace visible cuánto lo bloquean**, que es el número que
justifica las fases siguientes.

**Entregable independiente.** Aunque el plan se detenga acá, el sistema pasa a
reportar honestamente su propio incumplimiento de cadencia.

**Compuerta.** El sobrepaso de deadline aparece en métricas. Con inferencia
prendida, el reporte debe mostrar el atraso que hoy se esconde en el `min 1ms` de
la ráfaga de recuperación.

---

## Fase 2 — El puerto de observabilidad

**Qué.** [ADR-035](docs/adrs/035-observability-port.md): trait `VizSink`,
`VizBridge` como implementación directa, `VizRelay` con hilo propio y slot,
`FanoutObserver.viz` a `Box<dyn VizSink>`.

**Por qué segundo.** Es el borde que ya causó daño medido —- 8,2 s de scan— y el
de mejor relación daño-evitado por esfuerzo. Los trece sitios de llamada no se
tocan.

**Compuerta.** `02-ingest-viz`, variante `a-raw-native` (la que satura el
enlace a propósito), corrida larga y repetida: cero overruns de ciclo y cero
keyframes perdidos, con el contador de frames pisados del slot subiendo. **La
saturación debe volverse invisible para el lazo y visible en las métricas.**

Esa variante es hoy no determinista: degrada en unas corridas y en otras no. Esta
fase la convierte en determinista por el lado correcto.

---

## Fase 3 — Percepción a su hilo

**Qué.** Decode e inferencia salen del lazo. `Slot<RawKeyframe>` a la entrada,
`Slot<ProcessImage>` a la salida.

**Por qué tercero.** Es la fase que hace que el sistema cumpla lo que la wiki
promete: 216 ms de inferencia dejan de perturbar la cadencia clínica.

**Riesgo alto y por qué.** Es donde el `App` de 19 campos se parte de verdad. La
mitigación es que los campos **ya están agrupados por subsistema**
(`ARCHITECTURE.md` §3): el corte sigue una línea que ya existe.

Acá cae también la carga de modelos con inferencia apagada, que queda natural al
separar la etapa.

**Compuerta.** Un escenario nuevo, `03-ingest-infer`, con inferencia prendida:
p95 de periodo de ciclo dentro de ±10 ms de los 200 ms nominales, y el `min`
dejando de mostrar ráfagas de recuperación.

---

## Fase 4 — Ingesta a su hilo

**Qué.** RTSP, demux y dedupe pasan a etapa propia. Desaparece el `select!` del
camino caliente.

**Por qué acá.** Es la fase que **elimina la clase de bug**, no un bug. Sin
`select!` no hay cancelación, y el invariante de cancelación-seguridad
(`ingest::tests::staged_keyframe_survives_cancellation`) deja de ser una regla
que alguien tiene que recordar.

El test se conserva igual: documenta por qué la estructura es como es.

**Compuerta.** `01-ingest-only` en verde con `poll_timeout_ms` en valores
adversos —- justamente los que hoy hacen caer la ingesta a cero.

---

## Fase 5 — El reloj y la degradación

**Qué.** Dos cosas que sólo tienen sentido con el lazo ya aislado:

1. **Reloj de control derivado del reloj monotónico** en vez de contar ticks. Se
   borra la dependencia silenciosa de `MissedTickBehavior::Burst`
   (`ARCHITECTURE.md` §2.3). Enmienda el mecanismo de
   [ADR-029](docs/adrs/029-injected-clock.md), no su principio.
2. **El presupuesto actúa**, no sólo cuenta. Un overrun sostenido degrada algo
   —- bajar calidad de viz, apagar el sink— o escala como señal de salud que el
   FSM pueda consumir.

**Por qué al final.** El punto 2 requiere que un overrun signifique algo preciso.
Hoy, con todo compartiendo hilo, un overrun no dice de quién es la culpa. Con las
etapas separadas, sí —- y recién ahí se puede decidir qué degradar.

**Compuerta.** Un escenario de degradación inducida: saturar el enlace a
propósito y verificar que el sistema se degrada de forma declarada y observable,
en vez de atrasarse en silencio.

---

## Resumen

| Fase | Entrega | Riesgo | Compuerta |
|---|---|---|---|
| 0 | Base verde | bajo | 01 y 02 en verde |
| 1 | El lazo reporta su propio atraso | bajo | sobrepaso visible en métricas |
| 2 | Viz no puede frenar el control | medio | `a-raw-native` determinista |
| 3 | La cadencia se cumple de verdad | **alto** | `03-ingest-infer`, p95 ±10 ms |
| 4 | La clase de bug de cancelación desaparece | medio | 01 con timeouts adversos |
| 5 | El sistema declara su degradación | medio | escenario de degradación inducida |

El orden no es por dolor —- si fuera por dolor, la Fase 2 iría primera. Es por
**capacidad de medir**: cada fase deja instalado el instrumento con el que se
verifica la siguiente.
