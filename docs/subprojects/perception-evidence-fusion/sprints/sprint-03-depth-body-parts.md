# Sprint 3: Depth Evidence For Body Parts

**Estado:** implementado como evidencia diagnostica

**Objetivo:** asociar una estadistica robusta de profundidad a cada parte
corporal derivada de pose y limitada por la mascara de segmentacion, sin
convertir todavia esa evidencia en una decision clinica de postura.

## Flujo

```text
detect/track
    +--> pose + segmentation -> BodyPartsEstimate
    +--> depth-standard      -> ROI de escena y cama
    +--> depth-person        -> crop de bbox del track
                                  |
                                  v
                    huellas de body parts + mascara
                                  |
                                  v
                         PolygonStats por parte
```

`depth-person` es opcional y usa `largest_class` con margen. Si no existe una
salida depth para el track, el estimador usa el depth root con ROI fija. El
depth de escena no se elimina: es la referencia necesaria para cama y entorno.

## Contrato

Cada `body_parts` incluye, cuando hay muestras validas:

- `source_model`, `roi`, `map_width`, `map_height`;
- `sampled_pixels`, `valid_pixels`, `valid_ratio`;
- `min_depth_m`, `median_depth_m`, `p10_depth_m`, `p90_depth_m`, `max_depth_m`;
- `relative_to_torso_m` cuando torso y parte fueron muestreados en el mismo
  mapa.

Las estadisticas usan la union de huellas. Las polilineas se convierten en
capsulas y las geometrias se recortan contra los contornos normalizados de la
segmentacion. El muestreo transforma el centro de cada pixel del mapa a
coordenadas globales, por lo que no asume que el mapa tenga la misma resolucion
que el ROI.

## Limites

- La profundidad permanece como evidencia de percepcion y no entra al FSM.
- `relative_to_torso_m` no se compara entre crops distintos sin alineamiento.
- Las referencias calibradas de cama/piso deben aplicarse en una etapa posterior
  de plano 3D o residual calibrado.
- La regla actual de la cascada exige una persona confirmada y visible; la
  asociacion multi-persona por track requiere una regla especifica posterior.

## Siguiente sprint

1. incorporar referencias calibradas de cama/piso y residual de plano;
2. combinar pose, mascara, depth y persistencia para postura;
3. detectar una extremidad fuera de cama con hysteresis temporal;
4. validar umbrales con escenas etiquetadas antes de activar decisiones de
   control.
