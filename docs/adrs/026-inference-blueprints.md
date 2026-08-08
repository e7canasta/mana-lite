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

`mana.toml` selecciona exactamente el blueprint activo mediante
`inference.blueprint_file`.

El catalogo de modelos sigue siendo compartido. Al cargar un blueprint, el
runtime crea una vista runtime del catalogo y habilita solo sus modelos. Esto
permite que un mismo `models.toml` soporte varias combinaciones sin editarlo
para cada despliegue.

## Activation policies

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
