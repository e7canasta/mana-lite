# Subproyecto: Scheduler Cooperativo de Inferencia

**Estado:** Sprint 1 verificado; Sprint 2 instrumentado y listo para corrida de hardware
**Fecha:** 2026-08-12
**Alcance:** cadencia por modelo, latest-wins y ejecucion cooperativa dentro del hilo de percepcion

## Objetivo

Permitir que cada modelo tenga una cadencia operativa propia sin introducir
workers por modelo ni modificar el lazo clinico aislado.

Un modelo caro puede ejecutarse cada varios segundos mientras el detector base
corre con cada keyframe elegible. Si la percepcion se atrasa, el sistema no
procesa backlog: toma el keyframe mas fresco del slot y deja que las metricas
declaren la degradacion.

## Big picture

```text
                         CONFIGURACION
       catalogo/overlay: path, task, imgsz, device, half
       blueprint:        modelos, gates, intervalo, prioridad
                                  |
                                  v
 RTSP -> ingest task -> Slot<RawKeyframe> -> hilo percepcion
                                                |
                                      +---------+---------+
                                      | scheduler         |
                                      |                   |
                                      | due por modelo    |
                                      | gates de cascada  |
                                      | urgent requests   |
                                      +---------+---------+
                                                |
                                detect -> face -> pose -> seg
                                                |
                         Slot<PerceptionOutput> + cola de eventos
                                                |
                                                v
                                  hilo control @ cadencia fija
                                                |
                                  Slot<ControlDirective>
                                                |
                                                +-- modelos activos
                                                +-- solicitudes urgentes
```

El diagrama describe una sola etapa de percepcion cooperativa. Los modelos no
son workers independientes en esta fase.

## Principios

- El control sigue siendo fijo y no espera a percepcion.
- La percepcion se despierta por llegada de keyframes, no por un timer propio.
- Un intervalo de modelo es un minimo entre inicios de ejecucion, no una
  garantia de frecuencia.
- No hay catch-up: una tarea atrasada no dispara varias inferencias consecutivas.
- Un slot conserva la muestra mas fresca y cuenta lo que pisa.
- Un resultado reutilizado conserva su `frame_number` y su edad.
- Una urgencia es cooperativa: no interrumpe una llamada ONNX en curso.
- El tamano tecnico del modelo pertenece al catalogo u overlay; la cadencia
  pertenece al blueprint operativo.

## Documentos

- [Memoria tecnica](technical-memory.md): contexto, decisiones, limites y
  consecuencias.
- [Especificacion](spec.md): contrato funcional y criterios verificables.
- [Roadmap](roadmap.md): big picture, fases y puertas de decision.
- [Sprint 1](sprints/sprint-01.md): primera entrega de cadencia normal.
- [Sprint 2](sprints/sprint-02.md): instrumento de capacidad y corrida larga.
- [Handoff Sprint 3](sprints/sprint-03-handoff.md): contrato, plan y puertas de
  entrada para urgencias cooperativas.

## Fuentes de arquitectura

- [ADR-016: Cascade Scheduler](../../adrs/016-cascade-scheduler.md)
- [ADR-026: Named Inference Blueprints](../../adrs/026-inference-blueprints.md)
- [ADR-033: Lazo de control aislado](../../adrs/033-isolated-control-loop.md)
- [ADR-034: Slots y colas](../../adrs/034-slots-and-queues.md)
- [ADR-032: Senales de escena como contrato](../../adrs/032-scene-signals-as-contract.md)

## No objetivos de la primera entrega

- No crear un hilo por modelo.
- No introducir batching ni procesamiento de P-frames.
- No hacer preempcion de inferencia.
- No mover keypoints, masks o tipos internos al crate de control.
- No cambiar la semantica clinica de presencia, tracking o FSM.

## Criterio global de exito

Con una fuente de keyframes de `1 Hz`, un blueprint puede declarar, por ejemplo,
detector a `1 Hz` y segmentacion a `0.5 Hz`; la salida debe seguir siendo fresca
cuando existe capacidad, debe descartar muestras viejas cuando no la existe y
debe mostrar en metricas si la percepcion no alcanza la fuente.
