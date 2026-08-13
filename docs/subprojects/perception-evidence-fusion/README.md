# Subproyecto: Fusion de Evidencias y Estimador de Partes Corporales

**Estado:** Sprint 3, con profundidad por body part en modo diagnostico opt-in
**Alcance:** validacion cruzada entre modelos y estimacion geometrica de partes
**Dependencias:** cascada cooperativa, consolidacion y tracking existentes

## Proposito

Explorar dos capacidades relacionadas, pero separadas:

1. **Cross-model validation:** medir cuanto se apoyan o contradicen las
   evidencias de `detect`, `face`, `pose` y `segment`.
2. **Body parts estimator:** construir una estimacion geometrica de partes
   corporales usando detecciones ya inferidas, keypoints, face, mascara y tiempo.
3. **Depth evidence:** muestrear el mapa depth dentro de cada huella corporal
   y conservar profundidad relativa al torso para la siguiente etapa postural.

La primera responde: **"que tan coherentes son las evidencias?"**

La segunda responde: **"dada esta evidencia, como estimamos la geometria de
cabeza, tronco, brazos y piernas?"**

Ninguna de las dos convierte la cascada en un conjunto de modelos que se llamen
directamente entre si. El scheduler sigue siendo el unico dueño de la ejecucion.

## Arquitectura conceptual

```text
raw model outputs
        |
        +--> association by actor and frame
        |          |
        |          +--> CrossModelValidation
        |          |       agreement, freshness, quality
        |          |
        |          +--> BodyPartsEstimator
        |                  parts, geometry, per-part quality
        |
        +--> temporal evidence window: t-n ... t

semantic summary --> control/FSM
diagnostics       --> JSONL/Rerun
raw keypoints and masks remain in perception
```

La confianza no es binaria. Un modelo con evidencia fuerte puede servir de ancla
para combinar las otras fuentes, pero no obtiene autoridad absoluta: la calidad
final incorpora confianza de origen, acuerdo geometrico, frescura y estabilidad
temporal.

## Documentos

- [Memoria tecnica](technical-memory.md): contexto, limites y decisiones de
  diseño.
- [Diseño de integración](implementation-design.md): flujo real, símbolos y
  plan de cambios por etapa.
- [Especificacion](spec.md): contratos, entradas, salidas y criterios
  verificables.
- [Roadmap](roadmap.md): sprints y puertas de decision.
- [ADR-001](adrs/001-separate-validation-and-estimator.md): separar validacion y
  estimacion.
- [ADR-002](adrs/002-confidence-weighted-evidence.md): fusion ponderada y no
  binaria.
- [ADR-003](adrs/003-body-parts-derived-geometry.md): geometria derivada por
  partes.
- [Sprint 0](sprints/sprint-00-charter.md): contrato y limites.
- [Sprint 1](sprints/sprint-01-cross-model-validation.md): validacion cruzada.
- [Sprint 2](sprints/sprint-02-body-parts-estimator.md): estimador de partes.
- [Sprint 3](sprints/sprint-03-temporal-evidence-fusion.md): ventana temporal completa.
- [Sprint 3 depth](sprints/sprint-03-depth-body-parts.md): profundidad por parte
  y crop opcional sobre bbox de persona.

## Relacion con la documentacion existente

- `docs/subprojects/cooperative-inference-scheduler/` ya documenta la cadencia,
  las urgencias y el contrato especializado face/pose.
- [ADR-017](../../adrs/017-detection-consolidation.md) define `Detection`,
  `ConsolidatedObservation` y la separación conceptual de una entidad
  trackeada. El tipo real del runtime es `mana_control::Track`.
- Este subproyecto no reemplaza esos contratos. Agrega una capa de evidencia
  derivada y mantiene las partes corporales fuera del puerto de control hasta
  que exista una necesidad operativa concreta.

## Punto de integración real

La evidencia rica debe ensamblarse desde `PendingModelOutput` en
`src/app/inference.rs`, antes de que la consolidación stateless descarte los
keypoints. La identidad disponible hoy es limitada: `CascadeTarget.id` para
children asociados a un track; el caso sin track solo puede ser frame-local.

El blueprint normal `detect-room-face` no habilita pose ni segmentacion. El MVP
de este subproyecto debe usar un blueprint que habilite explícitamente
`detect-fast`, `face-yolo`, `pose-standard` y `seg-standard`, preferentemente el
perfil `config/blueprints/detect-face-pose-seg/blueprint.toml`. Para probar
profundidad por partes se agrega el perfil opt-in
`config/blueprints/detect-face-pose-seg-depth/blueprint.toml`.

## No objetivos

- No crear un detector nuevo ni entrenar un modelo de partes.
- No modificar la mascara cruda producida por segmentacion.
- No exigir que todos los keypoints esten visibles.
- No usar una regla binaria de inclusion como unica decision.
- No mover keypoints, mascaras ni indices de joints al crate de control.
- No introducir ejecucion same-frame dinamica durante una inferencia, workers o
  preempcion como parte del primer corte. El soporte existente de
  `same_frame = true` no se elimina.
