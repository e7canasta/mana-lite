# ADR-PA-003: Geometria Primero y Profundidad como Complemento

**Status:** Accepted — implementado en el engine offline v0.2
**Date:** 2026-08-13

## Contexto

Las pruebas con l-640 muestran que un keypoint aislado puede desviarse mas que
la region corporal construida desde pose y mascara. Tambien muestran deriva de
profundidad cuando aparece una persona, aunque la escena no haya cambiado. La
cara puede caer en una banda de profundidad ambigua y no debe decidir la postura
por si sola.

## Decision

La decision de postura usa este orden:

1. geometria 2D y organizacion de keypoints;
2. BodyPartsEstimator y cobertura de mascara;
3. profundidad relativa al torso y a superficies de la escena;
4. face como ancla/refuerzo de `head`;
5. persistencia temporal cuando se implemente.

La profundidad se muestrea por area corporal cuando sea posible. Los puntos
individuales se conservan como evidencia, pero no vencen por defecto a una
mediana de area con buena cobertura.

`Metric` se conserva para la visualizacion porque usa directamente la salida del
modelo y mantiene una barra comun entre fondo, face y keypoints. Sus valores se
denominan profundidad del modelo en el contrato nuevo, aunque los artefactos
historicos puedan conservar el campo `depth_m`.

## Alternativas descartadas

- Clasificar por color o depth global: pierde la geometria del actor.
- Clasificar por face: falla en posturas laterales y en limites de cama.
- Clasificar por un unico joint: sensible a errores puntuales.
- Usar `in_envelope` como gate absoluto: la deriva de contenido lo vuelve
  demasiado estricto para el primer engine.
- Usar solo coordenadas verticales: la perspectiva y la postura lateral pueden
  producir el mismo `y` con estados distintos.

## Consecuencias

- El engine puede seguir funcionando cuando depth no este disponible.
- El depth mejora la separacion entre torso en cama, piernas fuera y cuerpo en
  piso, pero conserva un estado de incertidumbre cuando las zonas se solapan.
- Se necesita conservar coordenadas, geometrias, cobertura y profundidad en el
  JSON para auditar una decision.
- Los pesos deben calibrarse con capturas nuevas y no fijarse por intuicion.
