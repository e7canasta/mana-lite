# ADR-027: Tier Architecture by Determinism Class

**Status:** Accepted
**Date:** 2026-08-09

## Context

ADR-001 fija el artefacto (un binario) y ADR-003 fija el modelo de ejecución
(superloop PLC). Ninguno de los dos dice **dónde vive cada pieza de código**.
El resultado es que la ubicación de un módulo se decidió caso por caso, y hoy
existen dependencias que contradicen el modelo de ejecución: percepción lee
estado del tracker, el lazo de control escribe JSONL.

Los intentos previos de ordenar esto usaron vocabulario de Domain-Driven
Design (bounded contexts, agregados, domain events). Ese registro es el
equivocado: DDD parte por sustantivos de negocio, y aquí no hay negocio —
hay un lazo de control con dos tasas de reloj. La partición correcta es por
**clase de determinismo**, como en cualquier middleware de control.

## Decision

El sistema se organiza en cuatro tiers definidos por tasa, determinismo y
semántica de fallo.

| Tier | Rol | Tasa | Semántica de fallo |
|---|---|---|---|
| **T0 · álgebra** | Tipos y matemática pura. Sin estado, sin reloj, sin tasa. | — | no falla |
| **T1 · campo** | Sensado y E/S: RTSP, decode, ONNX. Latencia no acotada. | variable | **fallar es normal** |
| **T2 · programa** | Tracker, presencia, ocupación, zonas, FSM, health. | **fija** | **no puede fallar: tickea siempre** |
| **T3 · reporte** | JSONL, métricas, Rerun. Fuera del camino crítico. | best-effort | falla en silencio; nunca bloquea el tick |

Entre T1 y T2 hay exactamente un objeto: la **imagen de proceso**
(`ProcessImage`), congelada, fechada y con edad explícita.

### Regla de pertenencia

Un componente pertenece a T2 si y solo si la respuesta es afirmativa:

> **Si la entrada nunca vuelve a llegar, ¿este componente tiene que seguir
> produciendo salida correcta en cada tick?**

Sí → T2. No → T1.

Esta pregunta reemplaza al juicio arquitectónico caso por caso. No es
teórica: el FSM ya tiene un estado `blind` que se recupera vía guard
`data_fresh`, y `Health` ya emite heartbeat de recuperación. El invariante ya
está implementado; este ADR lo convierte en regla de ubicación.

### Dirección de dependencia

**T2 no depende de T1.** No por estética: porque T2 debe seguir corriendo
cuando T1 está muerto. Un `mana-control` que no compila sin `mana-perception`
es un lazo de control que no puede sobrevivir a una cámara caída.

### Propiedad de la imagen de proceso

`ProcessImage` pertenece a **T2**, no a T1. Un PLC es dueño de su imagen de
proceso; los dispositivos de campo no saben que existe. El adaptador del
runtime (T3/binario) la construye a partir de la salida de percepción;
percepción nunca nombra el tipo.

## Consequences

- **Positivo:** La ubicación de cualquier archivo, tipo o función se decide
  con una sola pregunta, sin debate.
- **Positivo:** La regla es verificable en tiempo de compilación para todo lo
  que sea una dependencia (ver ADR-028).
- **Positivo:** El requisito clínico (seguir decidiendo con el sensado muerto)
  queda expresado como propiedad estructural, no como disciplina.
- **Negativo:** Obliga a un adaptador explícito en el binario entre percepción
  y control. Es código que antes no existía porque el acoplamiento era directo.
- **Negativo:** Hay invariantes de tier que ninguna frontera de crate puede
  hacer cumplir — en particular el acceso al reloj de pared. Ver ADR-029.

## References

- ADR-001 (single binary), ADR-003 (PLC superloop)
- ADR-028 (crate boundaries), ADR-029 (injected clock)
