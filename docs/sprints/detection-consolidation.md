# Sprint: Detection Consolidation

**Duración estimada:** 5-7 días
**Objetivo:** convertir salidas de múltiples modelos en entidades semánticas
únicas y mantener sus evidencias entre frecuencias de inferencia distintas.

## Scope

```text
Detection per model
  ↓
Per-model NMS + filters
  ↓
ConsolidatedObservation consolidation
  ↓
TrackedEntity temporal identity
  ↓
Cascade target selection
  ↓
Entity evidence enrichment
```

## Work Breakdown

### 1. Contracts and Types

- Añadir `ModelKey`, `ClassName`, `Bbox` y `TrackId` como aliases/tipos de
  dominio donde aporten claridad.
- Añadir `DetectionPayload` para reservar keypoints, face y mask sin ampliar
  todos los consumidores.
- Crear `ConsolidatedObservation`, `DetectionEvidence` y `ComponentEvidence`.
- Mantener `Detection` como salida efímera y sin identidad.

**Salida:** tipos compilables, sin cambiar todavía el ciclo de inferencia.

### 2. Spatial Consolidator

- Crear `DetectionConsolidator` por ciclo.
- Fusionar detecciones de la misma clase con IoU configurable.
- Asociar face, pose y segment como componentes, no como entidades primarias.
- Preservar el bbox y confianza del modelo primario.
- Emitir estadísticas: fusionadas, enriquecidas, huérfanas y ambiguas.

**Salida:** una escena sintética con una persona, pose y face produce una sola
`ConsolidatedObservation`.

### 3. Tracking Integration

- Cambiar el input del tracker de detecciones a `ConsolidatedObservation`.
- Mantener `track_id` solo en `TrackedEntity`.
- Asociar observaciones nuevas al track existente por clase y IoU.
- Dejar TTL de evidencias secundarias para la etapa de tracking.
- Mantener `current_tracks()` como única fuente para la cascada.

**Salida:** la misma persona conserva `track_id` aunque pose o face aparezcan
en ciclos diferentes.

### 4. Cascade Integration

- La cascada consume `TrackedEntity`, no `Detection`.
- `requires_class`, área y región se evalúan sobre el bbox canónico.
- `pose` y `face` se ejecutan sobre el track seleccionado.
- Las nuevas detecciones hijas se incorporan como evidencia al track correcto.
- Eliminar el NMS cross-model como mecanismo de identidad.

**Salida:** un modelo hijo enriquece un track existente y no crea duplicados.

### 5. Observability and Viz

- JSONL: conservar eventos `detection` de diagnóstico y añadir
  `consolidated_detection`; reservar `entity` y `entity_evidence` para tracking.
- Rerun: dibujar bbox canónico una sola vez.
- Rerun: keypoints, face y masks en paths separados.
- Publicar counters de fusión, asociación fallida, evidencia expirada y NMS.

**Salida:** el operador ve una persona, no una caja por modelo.

### 6. Tests and Calibration

- Dos detectores con la misma persona producen una entidad.
- Persona + face produce una entidad con componente face.
- Persona + silla de ruedas no se fusionan.
- Pose retrasada se asocia al track correcto.
- La consolidación no tiene TTL ni elimina observaciones entre frames; ese caso
  pertenece a la etapa de tracking.
- Dos personas cercanas no se fusionan incorrectamente.
- Validar con video real y ajustar IoU, containment y TTL.

## Definition of Done

- Existe una entidad canónica por sujeto primario.
- `track_id` se asigna a entidades, no a salidas individuales de modelos.
- Rerun no dibuja dos bbox de persona para el mismo sujeto.
- JSONL conserva trazabilidad de modelo y entidad.
- La cascada funciona aunque child models tengan menor frecuencia.
- Todos los tests unitarios de consolidación pasan.

## Out of Scope

- Re-identificación por apariencia.
- Fusión de personas y objetos distintos sin una relación explícita.
- Kalman/Hungarian completo, salvo que el tracker actual sea insuficiente para
  los tests multi-persona.
