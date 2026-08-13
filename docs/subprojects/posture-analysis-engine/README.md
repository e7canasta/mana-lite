# Subproyecto: Engine de Analisis de Postura por Firmas

**Estado:** engine offline operativo; parser, perfiles, superficie y consenso por imagen disponibles
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
                                       +--> evaluador por perfil
                                       +--> atencion head/torso/legs
                                       +--> consenso por postura base
                                       +--> JSON explicable
```

## Posturas iniciales

La primera matriz se limita a estas muestras. No se agregan clases nuevas hasta
que existan capturas suficientes para validarlas:

| ID | Interpretacion inicial |
|---|---|
| `acostado-1` | base `acostado`, plano `in-bed` |
| `sentado-1` | base `sentado-in-bed`, plano `in-bed` |
| `sentado-borde-1` | base `sentado-aside`, plano `aside-bed` |
| `parado-aside-1` | base `standby-aside`, plano `aside-bed` |
| `leaving-bed-aside-head-1` | variante de `acostado`, plano `in-bed` |
| `foot-left-bed-2` | variante de `acostado`, plano `in-bed` |
| `foots-left-bed-1` | variante de `acostado`, plano `in-bed` |

## Principios

- La geometria del cuerpo es la senal primaria: bbox, keypoints y partes.
- Face bbox, keypoints de cabeza y torso son la atencion fuerte; caderas, piernas
  y pies refinan la separacion entre sentado y acostado.
- La mascara limita las areas corporales, pero no reemplaza pose.
- La profundidad `Metric` se interpreta como valor relativo del modelo, no como
  distancia fisica garantizada.
- La calibracion bed/floor se usa espacialmente sobre poligonos con padding;
  `head/body/feet` son cuadrantes blandos y adyacentes, no fronteras binarias.
- Las anclas de torso y caderas combinan posicion, `zone` y `delta_m` calibrados;
  el ajuste de profundidad confirma la evidencia pero no la bloquea por si solo.
- Face refuerza la cabeza, pero sus bbox/zone se fusionan con keypoints de cabeza;
  si falta face, la evidencia de pose puede sustituirla con calidad parcial.
- La ausencia de una fuente no es una contradiccion.
- Una parte parcial reduce su peso; no invalida automaticamente al actor.
- La discrepancia entre fuentes se publica como razon, no se oculta en un score.
- Las firmas tienen rangos blandos y tolerancias; no son copias exactas de una
  imagen de entrenamiento.
- Cada perfil es un evaluador final independiente. El consenso posterior agrupa
  variantes en una postura base y conserva el ranking de ambos niveles.
- El resultado `unknown` es valido y preferible a una clasificacion forzada.

## Documentos

- [Memoria tecnica](technical-memory.md): evidencia actual y marco de pensamiento.
- [Manual operativo](manual.md): como ejecutar una prueba y leer su resultado.
- [Especificacion](spec.md): contrato funcional de entradas, scoring, consenso y salida JSON.
- [ADR-PA-001](adrs/001-master-and-posture-profiles.md): calibracion y perfiles.
- [ADR-PA-002](adrs/002-partial-evidence-quorum.md): evidencia parcial y quorum.
- [ADR-PA-003](adrs/003-geometry-first-depth-complement.md): geometria primero.
- [ADR-PA-004](adrs/004-soft-signatures-no-autolearn.md): firmas blandas y no autoaprendizaje.

Los documentos de sprint, roadmap y onboarding fueron destilados en el manual,
la spec, la memoria tecnica y los ADRs. No forman parte del contrato vigente.

## Relacion con deep-fusion

Este subproyecto consume los contratos ya definidos por
`perception-evidence-fusion`:

- `SurfaceCalibration` y `SurfaceZone` de la capa de calibracion.
- `BodyPartsEstimator` y geometria parcial de percepcion.
- `DepthEvidence` por parte del pipeline de profundidad.
- Validacion cross-model como fuente de calidad, no como clasificador.

La capa de postura no mueve keypoints, mascaras ni poligonos internos a
`mana-control`. Solo podria publicar un resumen semantico estrecho despues de
una validacion temporal especifica.

## Artefactos previstos

```text
config/posture-analysis/l-640/master.toml
config/posture-analysis/l-640/surface-calibration.toml
config/posture-analysis/l-640/<posture>.toml
runs/<run-id>/<frame-id>/radio.json
runs/<run-id>/<frame-id>/parts.json
runs/<run-id>/<frame-id>/posture.json
```

Los TOML de postura son perfiles versionables. Cada prueba bajo `runs/` conserva
los reportes de entrada, previews, decision y explicacion de cada score. No se
usan directorios temporales para resultados que deban auditarse.
