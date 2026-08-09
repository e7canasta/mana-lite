# ADR-026: Named Inference Blueprints

**Status:** Accepted
**Date:** 2026-08-08

## Context

`models.toml` contiene el inventario de modelos y tambien decisiones de
despliegue. `cascade.toml` contiene reglas de elegibilidad, pero no expresa de
forma clara cual es el perfil operativo que se quiere ejecutar. `fsm.toml`
puede restringir aun mas los modelos por estado.

Esto dificulta mantener configuraciones 24/7 y comparar dos despliegues:

```text
detect + face
detect + face + pose + seg
```

Tambien hace ambiguo si un child debe activarse por una deteccion aislada o por
una identidad temporal estable.

## Decision

Introducir blueprints nombrados en `config/blueprints/<name>/blueprint.toml`.

Un blueprint declara:

- su nombre y descripcion.
- el modelo primario.
- los modelos activos.
- las reglas de cascade.
- si requiere tracking.
- un overlay opcional de parámetros de modelos.

`mana.toml` selecciona exactamente el blueprint activo mediante
`inference.blueprint_file`.

El catalogo de modelos sigue siendo compartido. Al cargar un blueprint, el
runtime crea una vista runtime del catalogo y habilita solo sus modelos. Esto
permite que un mismo `models.toml` soporte varias combinaciones sin editarlo
para cada despliegue.

Cuando un despliegue necesita tuning propio, el blueprint puede declarar
`model_overlay = "models.toml"`. Ese archivo debe incluir
`extends = "../../models.toml"` y solo puede sobrescribir modelos existentes.
El runtime compone una vista derivada sin mutar el catálogo compartido.

## Activation policies

### Presence signal debounce and room cardinality

Before the classic tracker, the consolidated primary observations pass through
the temporal presence filter. It is a signal-quality policy, not identity
tracking. A valid single-person observation turns presence on; a short run of
empty valid inference ticks holds the last observation; multiple people are
ambiguous immediately; an invalid tick does not count as absence.

```toml
[presence]
enabled = true
class = "person"

[presence.poi]
on_ms = 200
off_ms = 1600

[presence.occupancy]
single_confirm_ms = 3000
empty_confirm_ms = 8000
multiple_confirm_ms = 5000
multiple_exit_ms = 5000
require_confirmed_tracks = false
```

The POI signal policy and room-cardinality policy are deliberately separate.
The first is permissive and protects continuity for the person of interest. The
second uses monotonic TON/TOF timers and is independent of keyframe cadence:
`multiple` requires `multiple_confirm_ms` of valid evidence. A 24/7 policy may
additionally require two confirmed tracks; the raw calibration profile does
not. The cardinality state machine is visualized through Rerun `StateChange`
lanes and does not replace the clinical FSM for bed events.

The filter may feed a held observation to the classic tracker for a short
dropout. It does not create a new `track_id` and it does not replace IoU/Kalman
matching.

Se reconocen dos politicas:

### Same-frame

El child consume detecciones del parent del frame actual. No requiere
tracking. Es apropiado para calibracion y perfiles ligeros.

### Confirmed-track

El child consume un track confirmado y visible. Es apropiado para 24/7 porque
`min_hits` evita candidatos aislados y `misses = 0` evita ejecutar sobre
observaciones ausentes.

Un gate de una persona usa `requires_exact_count = 1`. Si hay cero o mas de una
persona elegible, el child se omite.

## Included blueprints

- `detect-face`: detector primario y face en el mismo frame.
- `detect-face-pose-seg`: detector primario mas face, pose y segmentacion,
  todos condicionados por un track confirmado de la unica persona visible.

## Why not make every runtime component configurable?

Ingest, decode, consolidation, tracking, zones, FSM y publish son fases
estructurales del binario. El blueprint solo configura el subgrafo de
inferencia. Esto conserva un runtime pequeno y verificable y evita convertir
TOML en un launcher generico.

## Consequences

- Cambiar de perfil 24/7 es cambiar una ruta en `mana.toml`.
- El catalogo deja de ser la unica fuente de activacion.
- Los children pueden probarse primero sin tracking y luego promoverse a un
  perfil estable con tracking.
- El esquema actual de `ModelEntry` sigue existiendo durante la migracion.
- La extraccion futura de `presets.toml` queda separada de la semantica del
  blueprint.

## Rejected alternatives

### Duplicar `models.toml` por despliegue

Rechazado porque mezcla artefactos con operacion, duplica paths y dificulta
comparar configuraciones.

### Mantener solo `cascade.toml`

Rechazado porque expresa gates pero no el conjunto operativo ni la seleccion
nombrada del modelo primario.

### Usar solo FSM para seleccionar modelos

Rechazado porque el FSM representa politica clinica. No debe ser el unico lugar
que defina el coste y el grafo de inferencia.
