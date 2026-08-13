# ADR-PA-001: Separar Calibracion Maestra y Perfiles de Postura

**Status:** Accepted — implementado en el engine offline v0.2
**Date:** 2026-08-13

## Contexto

La calibracion de profundidad describe una camara, una ROI y un modelo. Una
postura describe relaciones entre geometria corporal, mascara y superficies en
esa misma escena. Mezclar ambas cosas en un unico TOML dificulta saber si un
cambio proviene de la camara o de la clasificacion.

Ademas, una nueva captura no debe cambiar silenciosamente las referencias de
cama y piso ni las firmas que ya fueron auditadas.

## Decision

Se definen dos niveles de artefacto:

```text
SurfaceCalibration maestro
    -> contexto de camara, modelo, ROI y superficies

PostureProfile por postura
    -> features, centros, tolerancias, pesos y requisitos

PostureAnalysis
    -> observacion de una captura contra todos los perfiles
```

El TOML padre del engine referencia exactamente una sesion maestra y una lista
cerrada de perfiles. Cada perfil referencia su matriz de features, pero no
duplica los poligonos ni las estadisticas de cama/piso.

Un perfil solo es valido si coincide con `model_key`, fingerprint, ROI y
resolucion de la sesion maestra. Actualizar una firma requiere generar una nueva
version o una promocion explicita; el analisis nunca escribe los TOML.

## Alternativas descartadas

- Un unico TOML con calibracion y reglas clinicas: mezcla contextos y dificulta
  auditoria.
- Un perfil por postura con sus propias superficies: duplica referencias y
  permite inconsistencias entre posturas.
- Actualizacion online automatica: una captura erronea podria mover el baseline.
- Recalcular perfiles con profundidad de `depth-person`: su escala es local al
  crop y no es comparable con `depth-scene`.

## Consecuencias

- Se puede cambiar de postura sin recalibrar la camara.
- Se puede invalidar toda la matriz al cambiar el modelo o la ROI.
- El JSON puede señalar si el problema es contexto incompatible o evidencia
  insuficiente.
- La generacion de perfiles queda separada del analisis de nuevas capturas.
