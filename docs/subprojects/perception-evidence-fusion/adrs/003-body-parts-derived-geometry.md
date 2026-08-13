# ADR-003: Las Partes Son Geometria Derivada

**Status:** Accepted
**Date:** 2026-08-12

## Contexto

Pose ofrece estructura discreta mediante keypoints; face localiza la cabeza y
segmentacion ofrece una cobertura pixelada. El producto deseado es una
estimacion de partes, no necesariamente otro modelo ni una nueva mascara
canonica.

## Decision

`BodyPartsEstimator` construye partes a partir de geometria local:

- cabeza: face y joints de cabeza;
- tronco: hombros y cadera;
- brazos: hombro-codo-muneca;
- piernas: cadera-rodilla-tobillo;
- manos y pies: solo cuando haya evidencia suficiente.

Los segmentos se ensanchan con radios relativos al bbox y luego se refinan con
segmentacion cuando esta disponible. Se usan poligonos o capsulas por parte; no
se usa el convex hull global como unica geometria.

La mascara actual no tiene un unico espacio de coordenadas: sus poligonos se
normalizan al frame, mientras que `compact` conserva el espacio de mascara y
`origin`/`mask_dims` describen como ubicarlo. El estimador debe consultar la
representacion correcta y probar especialmente crops con offset antes de usar la
mascara para clipping.

La mascara cruda permanece intacta y se conserva como evidencia independiente.
La geometria derivada registra sus fuentes, calidad, frame y estado stale.

## Consecuencias

- Se pueden construir partes aunque una fuente este ausente o llegue en otro
  frame.
- La mascara puede corregir cobertura sin imponer una forma corporal rigida.
- Las oclusiones se representan como partes parciales o de baja calidad.
- La salida derivada no debe confundirse con una segmentacion ground truth.
- El primer corte publica las partes solo como diagnostico/Rerun; no las agrega a
  `SceneSample` ni a las senales del FSM.
