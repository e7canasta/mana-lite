# ADR-PA-004: Firmas Blandas y Sin Autoaprendizaje

**Status:** Accepted — implementado en el engine offline v0.2
**Date:** 2026-08-13

## Contexto

Las siete posturas actuales son muestras de referencia, no una base estadistica
suficiente para representar todas las personas, ropas, oclusiones y variaciones
de movimiento. Copiar sus valores como rangos duros sobreajustaria el engine.

Al mismo tiempo, aprender automaticamente de cada nueva captura destruiria la
trazabilidad: una clasificacion erronea podria convertirse en una firma.

## Decision

Cada feature se guarda como centro, tolerancia y peso. El soporte decrece de
forma continua al alejarse del centro. Los perfiles se generan o editan mediante
una operacion explicita y versionada. El analisis de una captura es solo lectura.

Un nuevo conjunto de capturas puede proponer una actualizacion, pero no la
promueve. La promocion requiere:

1. revisar el JSON explicable;
2. comparar candidatos y ambiguedades;
3. actualizar el perfil TOML;
4. repetir el replay de aceptacion.

## Alternativas descartadas

- Rangos exactos derivados de una unica imagen.
- KNN sin explicacion ni control de contexto.
- Autoajuste de tolerancias en runtime.
- Entrenar un clasificador antes de disponer de capturas etiquetadas y estados
  `unknown`.

## Consecuencias

- Las nuevas capturas sirven para mejorar el perfil sin contaminarlo.
- El JSON debe conservar la version del perfil usado.
- La primera fase sera mas conservadora y producira mas `ambiguous`/`unknown`.
- La tolerancia se podra ampliar por evidencia, no para ocultar contradicciones.
