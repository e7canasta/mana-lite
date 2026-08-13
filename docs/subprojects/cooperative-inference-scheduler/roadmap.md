# Roadmap: Scheduler Cooperativo de Inferencia

## Vision

Convertir el subgrafo de inferencia en un conjunto de tareas cooperativas,
medibles y ajustables por blueprint:

```text
modelo tecnico + perfil operativo
              |
              v
  elegibilidad temporal por modelo
              |
  gates + prioridad + frescura
              |
              v
  percepcion secuencial latest-wins
              |
  evidencia fechada y metricas
              |
              v
  tuning de modelos grandes sin tocar el control
```

## Estado de partida

- El control esta aislado y corre a cadencia fija.
- Percepcion tiene un hilo propio y procesa el keyframe mas fresco.
- La cascada ya tiene orden, dependencias, gates y crops.
- Los slots ya implementan latest-wins y cuentan overwrites.
- El gating temporal cooperativo y las urgencias de ADR-016 ya estan
  implementados; la validacion semantica face/pose sigue pendiente.
- Los blueprints ya son la frontera correcta para seleccionar perfiles.

## Fases

### Sprint 0: memoria y contrato

**Estado:** completado en esta entrega.

- carpeta del subproyecto;
- memoria tecnica;
- especificacion inicial;
- roadmap y big picture;
- alcance de Sprint 1;
- decision explicita de no usar workers en la primera version.

### Sprint 1: cadencia normal por modelo

**Estado:** completado.

- agregar `interval_min_ms` opcional a las reglas;
- mantener estado temporal por modelo;
- integrar la decision en roots y children;
- diferenciar no debido de gate de cascade;
- probar intervalo, atraso, latest-wins y compatibilidad por defecto;
- no agregar urgencias ni workers.

**Puerta de salida:** un blueprint puede ejecutar segmentacion a `0.5 Hz` sobre
una fuente a `1 Hz`, sin catch-up y con metricas honestas.

### Sprint 2: instrumento de capacidad

**Estado:** instrumentacion y corrida CPU `s/m` en `192/320` completadas.

- gaps reales por modelo: p50, p95 y max;
- atraso contra `next_due`;
- `not_due`, `due_but_gated`, `due_but_no_target`, `urgent` y expiraciones;
- ventana larga de reportes para modelos lentos;
- escenario de workshop con `detect + face + pose + seg` y perfiles `s/m` en
  `192` y `320`; `640` queda sólo como referencia histórica.

**Puerta de salida:** podemos distinguir un modelo lento de un modelo que corre
menos por politica o por falta de target.

### Sprint 3: solicitudes urgentes cooperativas

**Estado:** completado en `c902d69`; hardware `192/320` pendiente como
validacion operativa.

- contrato `InferenceRequest`;
- prioridad y expiracion;
- request persistente derivado de `ControlDirective`;
- request transitorio con cola o estado durable;
- consumo one-shot sin preempcion;
- limites anti-starvation y metricas.

La primera version ya tiene `InferenceRequest`, persistencia derivada de
`ControlDirective`, cola transitoria durable dentro de `CascadeScheduler`,
congelamiento por keyframe, prioridad, TTL, consumo one-shot y espera/expiracion
en metricas. El productor semantico face/pose queda deliberadamente para Sprint 4.

**Puerta de salida:** una request urgente puede pedir pose fuera de su periodo y
no se pierde ni puede bloquear el control. La semantica de incertidumbre queda
deliberadamente para Sprint 4.

### Sprint 4: validacion cruzada semantica

**Estado:** implementacion inicial y corrida CPU `192/320` completadas; politica
clinica pendiente.

- senales de percepcion para validacion face/pose;
- contrato estrecho hacia `mana-control`;
- `frame_number`, timestamp, calidad y edad;
- FSM generico que puede exigir validacion y consumir el resultado;
- fixtures de una alerta con validacion cruzada;
- catalogo local con copias fisicas de los artefactos ONNX.

La entrada operativa es
`sprints/sprint-04-handoff.md`. El primer corte recomendado produce la request
transitoria desde percepcion y deja al FSM como consumidor de las senales
semanticas; si se exige origen literal en el FSM, la decision y sus pruebas
estan explicitadas en el handoff.

**Puerta de salida:** una alerta puede exigir una segunda evidencia sin exponer
keypoints o masks al FSM.

### Sprint 5: same-frame dinamico y presupuesto

- evaluar si la validacion debe ocurrir en el mismo RGB;
- cola dinamica topologica para ejecutar un modelo solicitado por otro;
- limite de trabajo por ciclo de percepcion;
- politica de degradacion si varias urgencias compiten;
- solo si los datos justifican la complejidad.

**Puerta de salida:** same-frame se agrega por necesidad medida, no por
anticipacion.

### Sprint 6: tuning y escalera de modelos

- matrices de `s/m/l/x` y `192/320`;
- perfiles de cadencia por hardware;
- benchmark de 30 a 180 segundos por fuente;
- criterios de aceptacion sobre `kf_pisados`, edad de evidencia, p95 y max;
- documentacion operativa del perfil recomendado.

**Puerta de salida:** cada blueprint grande tiene una frecuencia defendible y
una razon documentada para su costo.

## Puertas de decision

- No pasar a workers por intuicion: exigir atraso sostenido del hilo de
  percepcion y beneficio medible.
- No reutilizar resultados sin edad y TTL.
- No llamar a un modelo desde otro modelo; usar una peticion del scheduler.
- No poner politica de modelo grande en el catalogo compartido si depende del
  despliegue; usar blueprint u overlay.
- No aceptar un nuevo knob sin una linea de metricas que lo haga verificable.

## Indicadores de salud

```text
salud de control:    scan deadline, cycle p95/max, overruns
salud de transporte: keyframe gap, keyframes dropped, kf_pisados
salud de evidencia:  evidence age p50/p95/max, blind
salud de modelo:     calls, Hz real, infer p50/p95/max, gaps, skips
salud de urgencia:   requests, wait, expirations, starvation
```

El numero clinico sigue siendo la edad de la evidencia. La frecuencia de un
modelo explica la salud del motor, pero no reemplaza esa medida.
