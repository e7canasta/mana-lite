# Sprint 3: Fusion Temporal de Evidencias

**Estado:** planificado
**Objetivo:** usar `t-n ... t` para estabilizar calidad y partes sin ocultar la
edad de cada evidencia.

## Alcance

- ventana acotada por actor/track;
- `EvidenceStore` y `PartHistory` propiedad de `PerceptionStage`;
- decaimiento por edad de evidencia;
- suavizado de keypoints y geometria por parte;
- amortiguacion de contradicciones aisladas;
- penalizacion de contradicciones persistentes;
- expiracion y reporte de partes stale.

## Politica temporal

- una evidencia nueva pesa mas que una vieja;
- la ausencia de un modelo en `t` no elimina automaticamente su ultima
  evidencia, pero la vuelve envejecida;
- una salida stale no se publica como fresca;
- no se procesa backlog de frames antiguos;
- la identidad temporal usa solo `Track(id)` inequívoco; un `FrameLocal` expira
  al terminar el keyframe;
- una contradiccion persistente reduce calidad aunque el modelo ancla conserve
  confianza alta.

## Casos de prueba

- pose intermitente con mascara estable;
- mascara intermitente con pose estable;
- face retrasada respecto a pose;
- cambio de actor en el mismo bbox aproximado;
- contradiccion de un frame y recuperacion posterior;
- contradiccion sostenida hasta expirar la estimacion.

## Criterios de aceptacion

- el estado se asocia al actor correcto;
- la edad de cada fuente es observable;
- la calidad temporal es reproducible;
- no hay backlog ilimitado ni catch-up de inferencia;
- una parte expirada desaparece o se declara ausente, no se recicla
  indefinidamente.
