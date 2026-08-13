# ADR-002: Fusionar Evidencias Con Pesos Continuos

**Status:** Accepted
**Date:** 2026-08-12

## Contexto

La confianza de face, pose y segmentacion no es directamente comparable. Ademas,
una evidencia con confianza alta puede contradecir la geometria de otra fuente.
Una regla del tipo "pasa/no pasa" perderia informacion y seria demasiado sensible
a un frame malo.

## Decision

La colaboracion entre modelos usa scores continuos y conserva dimensiones
separadas:

```text
source_confidence  confianza del modelo
agreement_quality  acuerdo geometrico con otras fuentes
freshness_quality  vigencia del frame
temporal_quality   estabilidad en la ventana
```

El diseño futuro seleccionara el ancla por confiabilidad ponderada, no por la
confianza cruda mas alta. Las otras fuentes pueden apoyar, completar o penalizar
la estimacion. Ningun modelo tiene autoridad absoluta.

Esto no describe una capacidad ya existente. Hoy el bbox canonico lo aporta el
modelo primario de la consolidacion, el target de la cascada puede seleccionarse
por area y la validacion face/pose especializada elige la face segun su confianza.
La migracion debe conservar esos caminos hasta que existan fixtures equivalentes
y una corrida de calibracion.

La ausencia de una fuente no es una contradiccion. Una contradiccion se registra
cuando existe evidencia suficiente para comparar y la relacion falla.

## Consecuencias

- Las salidas pueden expresar calidad parcial y razones de degradacion.
- La decision clinica puede exigir un umbral sin que la capa geometrica sea
  binaria.
- Los pesos y umbrales requieren calibracion por modelo, tamano y resolucion.
- La observabilidad debe distinguir ausencia, stale y contradiccion.

## Restriccion

Una calidad fusionada debe ser finita, estar en `[0, 1]` y conservar el origen y
la edad de las evidencias usadas para calcularla.
