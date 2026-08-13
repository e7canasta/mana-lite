# Especificacion: Fusion de Evidencias y Estimador de Partes Corporales

**Identificador:** SUBSPEC-002
**Estado:** contrato inicial, no implementado como subsistema independiente
**Version:** 0.1

## 1. Alcance

La especificacion define dos productos derivados de la percepcion:

1. `CrossModelValidation`, una evaluacion continua de coherencia entre
   evidencias del mismo actor.
2. `BodyPartsEstimate`, una estimacion geometrica por partes corporales.

Ambos consumen salidas ya producidas por la cascada. Ninguno ejecuta modelos.

## 2. Evidencia interna

El adaptador construye una vista minima desde `PendingModelOutput`; esos campos
no existen juntos en `Detection` y no deben agregarse todos al contrato de
`mana-control`:

```text
actor_ref
frame_number
frame_dimensions
source_model              # desde PendingModelOutput.model_key
source_confidence
bbox
face_bbox?
pose_keypoints? [x, y, confidence]
segmentation_mask?
observed_at               # reloj de percepcion al cerrar el keyframe
```

`Detection` conserva keypoints como `Vec<[f32; 3]>`; no existe un cuarto campo de
visibilidad. La visibilidad futura debe ser una decision explicita del adaptador
del modelo, no una suposicion sobre el tercer valor.

Las cajas y keypoints traducidos por el runtime estan en coordenadas del frame
original, incluso cuando el modelo se ejecuto sobre un crop. `DetectionMask` no
es homogenea: sus `polygons` se normalizan al frame, pero `compact` esta en el
espacio de mascara y requiere `origin`/`mask_dims`. Toda consulta de mascara debe
declarar su espacio.

`source_model`, `frame_number` y `observed_at` los agrega el adaptador desde
`PendingModelOutput.model_key`, el contexto del ciclo y el reloj de percepcion.

`actor_ref` tiene dos formas durante la migracion:

```text
Track(track_id)       # solo cuando CascadeTarget.id es inequivo
FrameLocal(frame, i)  # evidencia del keyframe, sin identidad temporal
```

El runtime actual no posee un `track_id` dentro de `Detection` ni dentro de
`ConsolidatedObservation`. No se debe fabricar identidad persistente por indice
de deteccion o por bbox mas grande.

## 3. CrossModelValidation

La salida interna debe ser equivalente a:

```text
CrossModelValidation {
    actor_ref
    frame_number
    quality                 # finite ratio [0, 1]
    agreement               # finite ratio [0, 1]
    freshness               # finite ratio [0, 1]
    supporting_sources
    contradicting_sources
    reasons
}
```

Reglas:

- comparar solo evidencias asociadas al mismo actor;
- separar `source_confidence` de `agreement`;
- producir `quality` continua, no solo `valid/invalid`;
- una evidencia ausente no equivale a una contradiccion;
- una contradiccion aislada no debe borrar una estimacion temporal estable;
- el resultado debe conservar `frame_number` y edad;
- si cruza a control, hacerlo mediante un resumen semantico estrecho;
- no cruzar keypoints, mascaras ni payloads internos al FSM.

## 4. Reglas geometricas iniciales

Las siguientes relaciones aportan evidencia, no gates duros:

| Relacion | Medida inicial |
|---|---|
| face-pose | distancia normalizada entre face y joints de cabeza |
| pose-segment | ratio de joints validos dentro de la mascara |
| segment-pose | exceso de mascara fuera del soporte de pose |
| face-person | containment y compatibilidad de bbox |
| temporal | continuidad de actor, posicion y calidad |

Los umbrales deben ser configurables y expresarse en proporciones del bbox o de
la resolucion, nunca como distancias magicas en pixeles.

Solo se compara geometria cuando ambas fuentes estan en estado `Observed`:

```text
NotScheduled | NotDue | Gated | NoTarget | Failed | RanEmpty | Observed | Stale
```

Los primeros cuatro estados describen scheduling/gates, `Failed` describe un
fallo operativo, `RanEmpty` describe una ejecucion sin salida y `Stale` describe
evidencia anterior vencida. Ninguno debe convertirse silenciosamente en una
contradiccion geometrica.

## 5. BodyPartsEstimate

La salida interna debe ser equivalente a:

```text
BodyPartsEstimate {
    actor_ref
    frame_number
    parts: [BodyPartEstimate]
    overall_quality
}

BodyPartEstimate {
    part                  # head, torso, arm, leg, hand, foot
    side?                 # left/right where applicable
    geometry              # points, capsule, bbox or polygon
    support               # pose, face, segment, temporal
    quality               # finite ratio [0, 1]
    source_frame_numbers
    stale
}
```

Reglas:

- una parte puede estar ausente o tener calidad baja;
- `head` puede usar face como ancla y pose como corroboracion;
- `torso` usa hombros y cadera como estructura primaria;
- brazos y piernas usan segmentos shoulder-elbow-wrist y hip-knee-ankle;
- manos y pies son opcionales y requieren joints suficientes;
- la mascara puede refinar el borde, pero el estimador no muta la mascara cruda;
- el soporte de una parte se calcula por segmentos locales, no por convex hull
  global;
- la salida conserva frescura y origen temporal de cada parte.

En el MVP, el sink JSONL publica `type=body_parts` con `actor_id` para un track
confirmado o `frame_local_index` para evidencia sin identidad, además de
`overall_quality` y la lista de partes. Cada parte conserva `geometry`,
`support`, `source_models`, `quality`, `mask_coverage`,
`source_frame_numbers` y `stale`. `stale` permanece en `false` hasta el Sprint 3.

## 6. Ventana temporal

La ventana se define como una secuencia acotada de observaciones del mismo actor:

```text
E(t-n), E(t-n+1), ..., E(t)
```

La fusion temporal debe:

- ponderar evidencia reciente mas que evidencia vieja;
- evitar catch-up de modelos;
- marcar evidencia stale en vez de presentarla como fresca;
- soportar ausencia de una fuente en un frame;
- no acumular backlog ilimitado.

La ventana temporal y el `EvidenceStore` son trabajo nuevo. El tracker actual
solo suaviza el bbox y `Track.evidence` conserva nombres de modelos, no frames ni
timestamps por fuente.

## 7. Observabilidad

Si una decision operativa lo justifica, registrar por ventana:

```text
cross_validation_attempted
cross_validation_supported
cross_validation_contradicted
cross_validation_stale
body_parts_estimated
body_parts_partial
body_parts_quality_p50/p95
```

No reutilizar `urgent`, `gated` o `not_due` para describir calidad semantica.

## 8. Criterios de aceptacion

- Face y pose del mismo actor producen una calidad reproducible.
- Face y pose de actores distintos no se validan por proximidad global.
- Ausencia de pose no se convierte en validacion negativa.
- Una mascara parcialmente visible no invalida toda la persona.
- La estimacion genera partes separadas y calidad por parte.
- Un joint oculto no se trata como joint visible.
- Una contradiccion puntual puede ser amortiguada por la ventana temporal.
- Una contradiccion persistente reduce la calidad aunque un modelo tenga
  confianza alta.
- Ningun modelo es llamado directamente por otro modelo.
- La mascara y los keypoints crudos permanecen fuera de `mana-control`.
- El MVP debe consumir la evidencia antes de `DetectionConsolidator`, porque
  `DetectionEvidence` y `ConsolidatedObservation` no conservan keypoints.
- El MVP fisico debe activar un blueprint con pose y segmentacion; el blueprint
  por defecto `detect-room-face` no basta.
- Las salidas derivadas conservan frame y frescura.
