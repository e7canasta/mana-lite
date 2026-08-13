# ADR-PA-002: Evidencia Parcial y Quorum por Componentes

**Status:** Accepted for construction
**Date:** 2026-08-13

## Contexto

En una instalacion real puede faltar face, un hombro, un tobillo o parte de la
mascara. La calidad de la observacion cambia por oclusion, crop, confianza y
perspectiva. Exigir todos los modelos produce falsos `unknown`; ignorar las
ausencias produce scores artificialmente altos.

## Decision

El engine fusiona componentes independientes:

```text
geometry
body_parts
depth
face
segment
```

Cada componente tiene estado, score, peso efectivo y razones. La ausencia de un
componente no es una contradiccion. El score final renormaliza los pesos solo
entre componentes observados y exige un quorum configurable de componentes y
features.

La calidad parcial se propaga:

- face ausente: `face=missing`, no se elimina `head` si pose y mascara lo
  sostienen;
- una pierna ausente: la otra pierna sigue participando;
- mascara parcial: la parte conserva profundidad, pero baja su peso por
  `mask_coverage`;
- un joint de baja confianza: no se trata como observado fuerte;
- un conflicto entre dos fuentes observadas: se registra y penaliza, no se
  convierte en ausencia.

El engine produce `classified`, `ambiguous` o `unknown`. No existe un fallback
silencioso a la postura con mayor score cuando el quorum no se cumple.

## Alternativas descartadas

- `AND` de face + pose + segmentacion: demasiado fragil ante oclusion.
- Promediar todos los valores disponibles sin estado: confunde ausencia con
  evidencia neutral.
- Elegir siempre la fuente con mayor confianza: confianza de modelo no equivale
  a acuerdo geometrico.
- Completar joints ausentes con valores inventados: crea precision falsa.

## Consecuencias

- El JSON debe explicar tanto lo que participo como lo que falto.
- Los umbrales de quorum son parte del perfil y se pueden probar offline.
- Una postura puede ser util con evidencia parcial, pero no si queda por debajo
  del minimo de observaciones.
- La temporalidad futura podra conservar una parte parcial sin presentarla como
  fresca si su fuente ya expiro.
