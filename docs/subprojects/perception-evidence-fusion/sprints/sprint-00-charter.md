# Sprint 0: Charter de Fusion de Evidencias y Partes

**Estado:** cerrado como documentacion inicial
**Objetivo:** fijar el problema sin mezclar validacion cruzada con estimacion de
partes.

## Entregables

- carpeta del subproyecto;
- memoria tecnica;
- especificacion inicial;
- roadmap;
- tres ADRs locales;
- separacion explicita de consumidores y fronteras de datos.

## Decisiones cerradas

- La validacion cruzada mide coherencia; no fabrica partes.
- El estimador de partes fabrica geometria; no ejecuta modelos.
- La confianza es continua y multidimensional.
- El tiempo `t-n ... t` aporta frescura y estabilidad, pero no borra la edad
  original de una evidencia.
- Los keypoints y mascaras permanecen en percepcion.

## Fuera de este sprint

- implementar el validador nuevo;
- elegir la formula de calidad definitiva;
- publicar poligonos en el FSM;
- calibrar thresholds con una camara concreta.

## Puerta de salida

Los Sprints 1 y 2 pueden avanzar en paralelo porque sus contratos son distintos:
el validador entrega calidad y razones; el estimador consume evidencia y entrega
partes.
