# Archivo — Aislar el lazo de control (2026-08-11/12)

Registro de ejecución de un trabajo **terminado**. No describe el sistema
actual: para eso está [ARCHITECTURE.md](../../../ARCHITECTURE.md).

## Qué se hizo

Cinco fases en dos días: sanear la línea base, medir el atraso del lazo, y
partir el pipeline en tres etapas con dueños de ejecución distintos.

El sistema pasó a cumplir su invariante —*corre a cadencia fija aunque el campo
esté muerto*— por primera vez. Las decisiones están en
[ADR-033](../../adrs/033-isolated-control-loop.md),
[ADR-034](../../adrs/034-slots-and-queues.md) y
[ADR-035](../../adrs/035-observability-port.md), con los números medidos adentro.

## Los números, en una tabla

Escenario 03, misma cámara y mismo modelo, antes y después de separar etapas:

| | antes | después |
|---|---|---|
| `cycle` p95 | 305–348 ms | **200–201 ms** |
| atraso del lazo, p95 | 101–154 ms | **1,4–3,3 ms** |
| vencimientos incumplidos | 5 por ventana | **0** |
| latencia de inferencia | 194–217 ms | 194–217 ms |

Escenario 02 `a-raw-native`, la corrida que antes tiraba la ingesta de video:

| | antes | después |
|---|---|---|
| scan bloqueado | **41 s** | — |
| reconexiones RTSP | 147 | **0** |
| keyframes perdidos | 24% | **0** |

## Lo que vale conservar del plan

**El orden fue por capacidad de medir, no por dolor.** Si hubiera sido por
dolor, la visualización iba primera. Cada fase dejó instalado el instrumento con
el que se verificó la siguiente, y eso es lo que permitió que la fase de mayor
riesgo se validara en una corrida en vez de por argumento.

**Tres fases no se construyeron como estaban planeadas y salió mejor.** La 2 se
resolvió por dónde quedó la frontera de ejecución en vez de por una abstracción.
La 5 perdió su premisa: no quedaba nada que degradar. Y el corte de la 3 resultó
ser un lazo cerrado y no un pipeline, cosa que dijo el código y no el plan.

Un plan que sobrevive intacto a su ejecución probablemente no se ejecutó contra
el código real.

## Los planes originales

- [Fase 0 — sanear la base](fase-0-sanear-base.md)
- [Fase 1 — la forma del lazo](fase-1-forma-del-lazo.md), con su cierre y las
  tres desviaciones al final
