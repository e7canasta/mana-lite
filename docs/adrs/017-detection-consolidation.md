# ADR-017: Detection Consolidation Across Models

**Status:** Accepted
**Date:** 2026-08-06

## Context

La cascada produce varias salidas que pueden describir el mismo sujeto:

```text
detect-fast   → person bbox
pose          → person bbox + keypoints
face          → face bbox
segment       → person bbox + mask
```

Si cada salida se publica como una entidad independiente, Rerun, JSONL y el
tracker muestran duplicados. Además, los modelos no necesariamente corren a
la misma frecuencia: pose y face pueden llegar varios ciclos después del
detector root.

NMS no resuelve todo el problema. NMS elimina candidatos solapados del mismo
tipo; no modela que una face está contenida en una persona ni que pose es
evidencia adicional de la misma entidad.

## Decision

Introducir tres niveles explícitos:

```text
Detection
  salida de un modelo en un ciclo

ConsolidatedObservation
  consolidación espacial, sin identidad temporal

TrackedEntity
  identidad temporal con evidencias de distinta frescura
```

La consolidación tendrá dos clases de relación:

1. **Fusion:** detecciones de la misma clase semántica describen el mismo
   objeto. Ejemplo: `person` de detect y `person` de pose.
2. **Enrichment:** una detección es un componente de otra entidad. Ejemplo:
   `face` dentro de `person`, o una máscara asociada al bbox de `person`.

El bbox canónico de una entidad lo aporta el modelo primario. Pose, face y
segment enriquecen la entidad y no crean otro bbox de escena publicado.

## Data Model

```rust
struct ModelDetections<'a> {
    model: &'a str,
    role: DetectionRole,
    detections: &'a [Detection],
}

struct Detection {
    model: ModelKey,
    class: ClassName,
    confidence: f32,
    bbox: Bbox,
    payload: DetectionPayload,
}

struct ConsolidatedObservation {
    class: ClassName,
    bbox: Bbox,
    confidence: f32,
    primary: EvidenceRef,
    evidence: Vec<EvidenceRef>,
    components: Vec<ComponentEvidence>,
}

struct TrackedEntity {
    id: TrackId,
    class: ClassName,
    bbox: Bbox,
    last_seen: Instant,
    evidence: EvidenceStore,
}
```

`ModelDetections` es una vista prestada del resultado del modelo. El
consolidator lee las detecciones y solo posee la evidencia necesaria para la
observación consolidada; no clona ni conserva la salida cruda completa.

`ConsolidatedObservation` no contiene `track_id`. La identidad solo aparece cuando
el tracker asocia observaciones entre ciclos.

## Association Rules

| Relationship | Association rule |
|---|---|
| Same class | IoU >= configured threshold |
| Face → person | Face coverage over its own bbox + compatible parent class |
| Segment → entity | Bbox IoU or mask coverage |
| Pose → person | Same class + IoU/containment; attach keypoints |
| Different primary classes | Never fuse implicitly |

Una silla de ruedas y una persona son entidades distintas salvo que una regla
semántica explícita defina una relación entre ellas.

## Multi-rate Semantics

Cada evidencia conserva su frescura:

```text
TrackedEntity 7
  bbox: detector       last_seen = t0
  pose: pose-standard  last_seen = t0 - 1.8s
  face: face-v12       last_seen = t0 - 0.4s
```

Cuando exista tracking, la entidad seguirá siendo válida mientras el bbox
primario cumpla la política del tracker. La expiración independiente de
evidencia secundaria mediante TTL queda como trabajo posterior; la
consolidación stateless no conserva frescura entre frames.

## Publication

La salida se divide en dos canales:

```text
model diagnostics       → detections por modelo, opcional/debug
consolidated_detection  → observación del frame, sin identidad
entity                  → identidad temporal, solo con tracking habilitado
```

En modo stateless, Rerun dibuja `ConsolidatedObservation` bajo
`/world/camera/observations`. Con tracking habilitado, además dibuja el bbox
canónico de `TrackedEntity` una sola vez y los enriquecimientos en capas
separadas: keypoints, face component y masks.

## Consequences

- **Positive:** una persona aparece una sola vez en la escena.
- **Positive:** pose, face y segment aportan información sin duplicar sujetos.
- **Positive:** el diseño deja espacio para frecuencia y TTL propios por
  evidencia en la etapa de tracking.
- **Positive:** el tracker opera sobre objetos semánticos, no sobre salidas de
  modelos competidores.
- **Negative:** se necesita una política explícita de asociación por relación.
- **Negative:** la publicación requiere distinguir diagnóstico, observación y
  entidad trackeada.

## Migration

La consolidación stateless ya reemplaza la deduplicación cross-model basada en
NMS. El siguiente paso es validar el tracker por separado, sin cambiar este
contrato ni descartar la evidencia de pose o face antes de asociarla.
