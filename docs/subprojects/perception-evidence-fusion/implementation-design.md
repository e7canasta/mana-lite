# Diseño de Integración: Fusion de Evidencias y Body Parts

**Estado:** Sprint 1 y Sprint 2 implementados inicialmente; temporalidad pendiente
**Fecha:** 2026-08-12

## 1. Dictamen

El punto correcto de integración es el hilo de percepción, después de que se
completen los modelos elegibles del keyframe y antes de que la consolidación
stateless descarte los keypoints.

```text
run_inference()
  -> PendingModelOutput[] completo
  -> EvidenceAssembler
  -> CrossModelValidator
  -> BodyPartsEstimator
  -> DetectionConsolidator
  -> ProcessImage / eventos / Rerun
```

No se debe reconstruir esta información desde `ConsolidatedObservation`:
`DetectionEvidence` y `ConsolidatedObservation` conservan bbox, máscara y
confianza, pero no keypoints, frame por fuente ni frescura independiente.

## 2. Lo que existe hoy

### Evidencia rica

`src/app/inference.rs` mantiene durante el keyframe:

```text
PendingModelOutput {
    model_key
    output: InferenceResult
    target: Option<CascadeTarget>
    crop_frame
    crop_rect
}
```

Es el único punto que reúne, sin perder información:

- modelo que produjo la salida;
- detecciones;
- keypoints;
- máscaras;
- target de cascada;
- contexto del crop.

El assembler debe consumir esta lista antes de `record_pending_results` y antes
de `DetectionConsolidator::consolidate`.

El primer assembler no persiste todavia un store: agrupa los outputs del
keyframe actual por `CascadeTarget.id` y entrega el resultado al evento debug
`cross_model_validation`. La persistencia por fuente queda para el Sprint 3.

### Identidad

`CascadeTarget.id` es un `track_id` cuando el child usa la directiva de control
anterior y el target viene de un track confirmado. No todas las evidencias tienen
ese ID:

- root del primer keyframe: `FrameLocal`;
- child `same_frame = true`: normalmente `FrameLocal`;
- child `same_frame = false`: puede ser `Track(id)`;
- varios actores: el blueprint actual suele exigir `requires_exact_count = 1` y
  el scheduler actual resuelve un target, no un vector completo.

Por ello el MVP debe limitarse a un actor confirmado o mantener evidencia
frame-local sin intentar convertirla en historial.

### Validación face/pose existente

`src/app/face_pose.rs` ya implementa el camino especializado:

- face incierta;
- request urgente transitoria para pose;
- contexto pendiente;
- validación en keyframe posterior;
- `FacePoseValidation { valid, quality, frame_number }` hacia control.

Este camino debe preservarse. El validador genérico no debe crear una segunda
request ni cambiar la semántica de `None` frente a `Some(valid = false)`.

### Control

`mana-control` recibe `SceneSample` y `AgedEvidence<SceneSample>`. No recibe
keypoints, máscaras ni geometría corporal. Ese límite es correcto para el MVP.

## 3. Tipos nuevos, todos internos a percepción

La primera implementación puede vivir en `src/app/evidence.rs`:

```text
ActorRef
  Track(u64)
  FrameLocal { frame_number, index }

EvidenceState
  NotScheduled | NotDue | Gated | NoTarget | Failed
  RanEmpty | Observed | Stale

SourceEvidence
  actor_ref
  source_kind
  model_key
  state
  frame_number
  observed_at
  confidence
  bbox
  keypoints: Option<Vec<[f32; 3]>>
  mask: Option<DetectionMask>
  association_basis

ActorEvidenceFrame
  actor_ref
  frame_number
  frame_dimensions
  sources
```

`SourceEvidence` es un tipo de adaptación. No es una razón para inflar
`Detection`, `DetectionEvidence`, `ConsolidatedObservation` o `SceneObservation`.
El clone de keypoints/máscara debe ser acotado y hacerse solo si el historial o
la visualización lo necesitan.

## 4. Ensamblado por keyframe

El algoritmo del primer corte:

1. Recibir `pending` completo del keyframe.
2. Convertir cada `PendingModelOutput` en fuentes con `model_key` y frame actual.
3. Usar `target.id` para formar `ActorRef::Track` cuando sea inequívoco.
4. Para `target.id == None`, usar `ActorRef::FrameLocal` y no guardar historial.
5. Asociar outputs del mismo target; no usar el bbox más grande como identidad.
6. Ejecutar relaciones disponibles solamente entre fuentes `Observed`.
7. Calcular `CrossModelValidation` por actor/frame.
8. Ejecutar `BodyPartsEstimator` con las fuentes y la calidad resultante.
9. Publicar diagnóstico y conservar el resultado semántico actual sin cambiarlo.

La consolidación existente continúa después de este paso y mantiene su contrato.

## 5. CrossModelValidator

Ubicación propuesta:

```text
src/app/cross_model_validation.rs
```

Entrada:

- `ActorEvidenceFrame`;
- configuración de pesos/thresholds;
- resumen temporal disponible, si el sprint ya lo implementó.

Salida:

```text
CrossModelValidation {
    actor_ref
    frame_number
    quality
    agreement
    freshness
    supporting_sources
    contradicting_sources
    reasons
}
```

Relaciones en orden recomendado:

1. `face-person`;
2. `face-pose` reutilizando la geometría de `src/app/face_pose.rs`;
3. `face-segment`;
4. `pose-segment` usando joints y polígonos de la máscara;
5. `segment-pose` midiendo exceso fuera del soporte local;
6. continuidad temporal cuando exista `Track(id)`.

La ausencia de un modelo, un gate o un intervalo no es contradicción. Un modelo
solo se compara si produjo evidencia suficiente en el frame correspondiente.

## 6. BodyPartsEstimator

Ubicación propuesta:

```text
src/app/body_parts.rs
```

MVP:

```text
head, torso, left_arm, right_arm, left_leg, right_leg
```

Derivación:

- cabeza: bbox de face como ancla; joints COCO de cabeza como corroboración;
- tronco: cuadrilátero entre hombros y caderas;
- brazos: cápsulas/polilíneas shoulder-elbow-wrist;
- piernas: cápsulas/polilíneas hip-knee-ankle;
- manos/pies: se posponen o se publican como puntos/regiones de baja calidad.

El mapa completo de joints debe fijarse en un adaptador y probarse. El tipo real
solo tiene `[x, y, confidence]`; no existe un cuarto campo de visibilidad.

La segmentación es una evidencia de cobertura. Puede:

- aumentar la calidad si contiene joints/segmentos;
- limitar un borde local;
- reducir calidad por exceso de máscara;
- dejar una parte parcial cuando la máscara es incompleta.

No puede crear por sí sola una etiqueta anatómica de brazo, tronco o pierna: la
segmentación actual es una silueta de persona.

### Espacios de máscara

`DetectionMask` mezcla representaciones:

- `polygons`: normalizados al frame por el runtime;
- `compact`: espacio de máscara/modelo;
- `origin` y `mask_dims`: metadatos para ubicar el espacio compacto.

La primera consulta de inclusión debe usar polígonos en frame y un test de crop
con offset. No convertir a una máscara full-frame por cada parte como primera
implementación.

## 7. Estado temporal

El modo `validator` sigue siendo stateless. El modo opt-in `advanced` tiene una
memoria acotada de geometría por `track_id`, propiedad de `PerceptionStage`:

```text
BodyPartsTemporalState:
  bounded map TrackId -> { bbox, last_seen_frame, part histories }
```

Cada parte histórica conserva sólo geometría, bbox de referencia, calidad, modelos
de origen y frame observado. En cada keyframe el modo avanzado:

- transforma la geometría histórica al bbox actual;
- la suaviza cuando la geometría actual es completa;
- completa una parte ausente o parcial sólo si la máscara actual la contiene;
- marca la parte recuperada como `stale` y aplica decaimiento de calidad;
- no usa memoria para actores `FrameLocal`.

La ventana completa de evidencia por fuente sigue siendo un trabajo posterior:

```text
EvidenceStore:
  bounded map ActorRef -> deque ActorEvidenceFrame
```

Propiedades:

- solo el hilo de percepción lo muta;
- máximo de actores y frames;
- TTL por fuente;
- poda en cada keyframe;
- no conserva RGB ni `InferenceResult` completo;
- conserva solo keypoints/máscara necesarios para validación y partes.

El tracker actual sólo suaviza bbox. La memoria avanzada de partes es
deliberadamente independiente y no modifica `mana-control`.

La actualización debe ser transaccional respecto al ciclo:

```text
calculate -> validate -> publish -> commit temporal
```

Si el pipeline captura un panic después de calcular, no debe quedar una ventana
temporal parcialmente actualizada.

## 8. Configuración y blueprint del MVP

El blueprint por defecto `detect-room-face` no habilita pose ni segmentación.
Para el primer workshop se debe seleccionar explícitamente:

```text
config/blueprints/detect-face-pose-seg/blueprint.toml
```

La política de los dos mecanismos vive en `config/mana.toml`, separada del
código de geometría:

```toml
[face_pose]
keypoint_min_confidence = 0.50
min_head_joints = 3
max_head_face_center_distance_ratio = 0.75
quality_face_weight = 0.25
quality_pose_weight = 0.20
quality_joint_weight = 0.30
quality_geometry_weight = 0.25

[perception.validation]
relation_support_threshold = 0.50
source_quality_weight = 0.40
agreement_quality_weight = 0.60

[perception.body_parts]
mode = "validator" # `advanced` habilita completado temporal respaldado por máscara
frame_local_match_iou = 0.50
face_frame_local_coverage = 0.50
joint_min_confidence = 0.25
segment_radius_ratio = 0.035
head_padding_ratio = 0.06
torso_radius_multiplier = 1.5
head_face_weight = 0.65
head_pose_weight = 0.35
mask_quality_weight = 0.25
cross_model_quality_weight = 0.20
minimum_geometry_extent_px = 1.0
geometry_epsilon = 0.00001
advanced_smoothing_alpha = 0.65
advanced_mask_support_threshold = 0.60
advanced_max_gap_frames = 3
advanced_temporal_quality_decay = 0.85
```

La validación de arranque rechaza valores no finitos, pesos sin suma positiva o
ratios fuera de rango. La memoria avanzada tiene un gap máximo acotado; el
EvidenceStore completo por fuente sigue reservado a una fase posterior.

## 9. Plan de cambios por fase

### Fase 1: evidencia y validator puro

Archivos nuevos:

```text
src/app/evidence.rs
src/app/cross_model_validation.rs
```

Archivos de integración:

```text
src/app/inference.rs
src/app/perception.rs
src/app/face_pose.rs
```

No tocar todavía `mana-control` ni el consolidado.

Esta fase ya esta implementada inicialmente. La validacion incluye face/pose,
pose/segment, face/segment y relaciones bbox disponibles; la salida no modifica
el sample clínico.

### Fase 2: body parts stateless

Archivo nuevo:

```text
src/app/body_parts.rs
```

Integrado después del validator en `run_inference()`. Publica primero al sink
JSONL de diagnóstico y no modifica el sample clínico.

### Fase 3: temporalidad

El primer corte avanzado ya agrega `BodyPartsTemporalState` a `PerceptionStage`,
con completado respaldado por máscara, `stale`, decaimiento, expiración y tests
deterministas. La ventana de evidencia por fuente y la reconstrucción anatómica
desde máscara sin anclas siguen fuera de este corte.

### Fase 4: resumen clínico opcional

Solo con una decisión concreta, proyectar un resumen estrecho a `SceneSample` y
agregar tags al catálogo de señales. Nunca transferir geometría interna al FSM.

## 10. Criterio MVP

El MVP queda acotado a:

- un actor confirmado;
- blueprint `detect-face-pose-seg`;
- evidencia del keyframe actual;
- face/pose/segmentación asociadas por `CascadeTarget.id`;
- partes head/torso/brazos/piernas;
- diagnóstico en Rerun/JSONL compacto;
- `FacePoseValidation` actual sin regresión;
- cero cambios al contrato clínico de partes.

Multi-actor, identidad temporal sin track, TTL por fuente y señales clínicas de
partes son fases posteriores, no supuestos del primer código.
