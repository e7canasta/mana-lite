# Subproyecto: Engine de Analisis de Postura por Firmas

**Estado:** especificacion inicial; engine offline pendiente de implementacion
**Alcance inicial:** siete posturas de `samples`, una camara fija y `depth-l-640`
**Dependencias:** `deep-calib`, `BodyPartsEstimator`, pose, face y segmentacion

## Proposito

Construir una capa de analisis que combine geometria 2D, partes corporales,
segmentacion y profundidad relativa para producir una postura explicable. La
primera version no toma decisiones clinicas ni modifica el FSM: genera un JSON
diagnostico que permite auditar cada senal, su calidad y los datos faltantes.

La hipotesis de trabajo es:

```text
calibrador maestro
        |
        +--> SurfaceCalibration de la camara y el modelo
        |
        +--> matriz de firmas por postura
                    |
captura --> detect/face/pose/seg/depth --> analizador
                                      |
                                      +--> score por componente
                                      +--> consenso o ambiguedad
                                      +--> JSON explicable
```

## Posturas iniciales

La primera matriz se limita a estas muestras. No se agregan clases nuevas hasta
que existan capturas suficientes para validarlas:

| ID | Interpretacion inicial |
|---|---|
| `acostado-1` | persona acostada |
| `sentado-1` | sentado sobre la cama |
| `sentado-borde-1` | sentado en el borde |
| `parado-aside-1` | parado al costado |
| `leaving-bed-aside-head-1` | saliendo por el lado de la cabecera |
| `foot-left-bed-2` | variante con un pie fuera |
| `foots-left-bed-1` | variante con pies fuera |

## Principios

- La geometria del cuerpo es la senal primaria: bbox, keypoints y partes.
- La mascara limita las areas corporales, pero no reemplaza pose.
- La profundidad `Metric` se interpreta como valor relativo del modelo, no como
  distancia fisica garantizada.
- Face refuerza la cabeza, pero nunca decide la postura por si sola.
- La ausencia de una fuente no es una contradiccion.
- Una parte parcial reduce su peso; no invalida automaticamente al actor.
- La discrepancia entre fuentes se publica como razon, no se oculta en un score.
- Las firmas tienen rangos blandos y tolerancias; no son copias exactas de una
  imagen de entrenamiento.
- El resultado `unknown` es valido y preferible a una clasificacion forzada.

## Documentos

- [Memoria tecnica](technical-memory.md): evidencia actual y marco de pensamiento.
- [Onboarding](onboarding.md): contexto comun, limites y primer paso del sprint.
- [Especificacion](spec.md): contrato de entradas, scoring y salida JSON.
- [Roadmap](roadmap.md): fases offline, replay y runtime.
- [ADR-PA-001](adrs/001-master-and-posture-profiles.md): calibracion y perfiles.
- [ADR-PA-002](adrs/002-partial-evidence-quorum.md): evidencia parcial y quorum.
- [ADR-PA-003](adrs/003-geometry-first-depth-complement.md): geometria primero.
- [ADR-PA-004](adrs/004-soft-signatures-no-autolearn.md): firmas blandas y no autoaprendizaje.
- [Sprint 5](sprints/sprint-05-posture-signature-engine.md): engine offline inicial.

## Relacion con deep-fusion

Este subproyecto consume los contratos ya definidos por
`perception-evidence-fusion`:

- `SurfaceCalibration` y `SurfaceZone` de Sprint 4.
- `BodyPartsEstimator` y geometria parcial de Sprint 2.
- `DepthEvidence` por parte de Sprint 3.
- Validacion cross-model como fuente de calidad, no como clasificador.

La capa de postura no mueve keypoints, mascaras ni poligonos internos a
`mana-control`. Solo podria publicar un resumen semantico estrecho despues de
una validacion temporal especifica.

## Artefactos previstos

```text
config/posture-analysis/l-640/master.toml
config/posture-analysis/l-640/<posture>.toml
demo-posture-analysis/l-640/<image>.json
```

Los TOML de postura son perfiles versionables. El JSON de analisis conserva las
observaciones de la captura y la explicacion de cada score.
