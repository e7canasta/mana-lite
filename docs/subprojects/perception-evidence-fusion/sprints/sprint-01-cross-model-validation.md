# Sprint 1: Cross-Model Validation

**Estado:** implementacion inicial y corrida fisica completadas; calibracion
pendiente
**Objetivo:** medir acuerdo entre evidencias del mismo actor sin convertirlo en
un gate binario global.

## Alcance

- formalizar un validador puro en el adaptador de percepcion;
- ensamblar desde `PendingModelOutput`, no desde `ConsolidatedObservation`;
- reutilizar `validate_face_pose` sin duplicar su request urgente;
- usar `CascadeTarget.id` solo cuando sea inequívoco;
- reutilizar la asociacion definida por ADR-017;
- cubrir face-pose, pose-segment y face-person;
- incluir confianza de origen, acuerdo geometrico y frescura;
- mantener el contrato semantico estrecho hacia control.

## Casos de prueba

- face y pose compatibles del mismo actor;
- face y pose incompatibles del mismo actor;
- face y pose de dos actores distintos;
- crop con coordenadas traducidas al frame original;
- joints insuficientes o no finitos;
- mascara parcial con pose valida;
- contradiccion aislada frente a contradiccion persistente;
- evidencia ausente frente a evidencia stale.

## Criterios de aceptacion

- `quality` es determinista, finita y esta en `[0, 1]`;
- se conserva `frame_number` y frescura;
- la mascara distingue `polygons` en frame de `compact` en espacio de mascara;
- la confianza mas alta no domina sin considerar acuerdo y tiempo;
- ausencia no se confunde con rechazo;
- no se mueven keypoints ni mascaras a `mana-control`;
- no se llama pose directamente desde face: se usa el scheduler cooperativo.
- el resultado actual usa `freshness = 1.0`; no pretende ser todavia una ventana
  temporal.

## Corrida Fisica

Perfil: `workshop/scenarios/11-inference-capacity/blueprint-m-192.toml`.

- 61 keyframes en la ventana de 60 s;
- 62 eventos `cross_model_validation` en JSONL;
- fuentes observadas: `face-yolo`, `pose-standard`, `seg-standard`;
- Rerun conectado a `192.168.1.24:9876`;
- 0 overruns y 0 deadlines perdidos;
- log: `/tmp/mana-quality/m192-cross-validation-sprint1/mana-20260813T01.jsonl`.

## No hacer

- ejecucion same-frame dinamica durante una inferencia; el modo estatico
  `same_frame = true` existente sigue siendo valido;
- nuevos workers;
- cambios de politica clinica del FSM;
- reutilizar metricas de urgencia como metricas de validacion.
