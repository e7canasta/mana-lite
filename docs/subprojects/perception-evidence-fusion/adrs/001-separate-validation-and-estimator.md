# ADR-001: Separar Validacion Cruzada y Estimador de Partes

**Status:** Accepted
**Date:** 2026-08-12

## Contexto

Face, pose y segmentacion describen al mismo actor desde perspectivas distintas.
Es tentador crear un unico componente que valide sus salidas y, al mismo tiempo,
fabrique poligonos de cabeza, tronco y extremidades. Eso mezcla una medida de
calidad con una construccion geometrica y hace dificil saber que resultado se
puede usar para una decision.

## Decision

Se definen dos componentes separados:

```text
CrossModelValidator
    entrada: evidencias asociadas
    salida: acuerdo, contradiccion, calidad y frescura

BodyPartsEstimator
    entrada: evidencias y calidad disponible
    salida: geometria derivada por parte y calidad por parte
```

El validador puede alimentar pesos del estimador, pero el estimador no cambia el
resultado de validacion retroactivamente. Ambos usan el mismo contrato de
evidencia y `frame_number`.

El store temporal no existe aun en el runtime. Cuando se implemente, sera
propiedad exclusiva de `PerceptionStage` en `src/app/perception.rs`, sin
`Mutex` ni ownership en `mana-control`. En el primer corte ambos componentes
pueden operar sobre la evidencia del keyframe actual y dejar la memoria
temporal para el sprint especifico.

## Consecuencias

- Se pueden probar geometria sin cambiar la politica de validacion.
- Una calidad baja puede producir una parte parcial, en vez de invalidar toda la
  observacion.
- Se distingue evidencia observada de geometria derivada.
- El control puede consumir un resumen de validacion sin recibir poligonos.
- Hay dos contratos que mantener, pero cada uno tiene un consumidor y una
  responsabilidad claros.
- La asociacion por actor es una capacidad parcial del runtime actual: el camino
  trackeado usa `PendingModelOutput.target.id`; el camino sin target solo es
  frame-local y no puede inventar identidad temporal.

## No decidido aqui

- La formula final de fusion de confianza.
- El formato de publicacion de poligonos parciales.
- Si alguna parte llegara a una senal clinica del FSM.
