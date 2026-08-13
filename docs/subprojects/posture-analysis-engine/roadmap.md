# Roadmap: Engine de Analisis de Postura por Firmas

## Vision

Convertir los reportes diagnosticos actuales en una decision explicable y
resistente a componentes parciales, sin acoplarla al scheduler ni al FSM antes
de disponer de replay y persistencia temporal.

```text
SurfaceCalibration maestro
        |
        v
perfiles TOML por postura
        |
        v
adaptador de reportes
        |
        +--> geometria 2D
        +--> BodyPartsEstimator
        +--> depth relativo
        +--> calidad y faltantes
        |
        v
consenso por imagen -> JSON explicable
        |
        v
ventana temporal -> resumen semantico
```

## Sprint 5: engine offline por firmas

Estado: documentado para comenzar.

- definir el TOML padre y los siete perfiles iniciales;
- adaptar los reportes `deep-calib-radio` y `deep-calib-parts`;
- calcular features normalizadas y scores blandos;
- producir candidatos, razones, faltantes y conflictos;
- validar determinismo, contexto y entradas parciales.

Puerta de salida: las siete muestras producen JSON explicable y ninguna fuente
ausente provoca un panic o una clasificacion forzada.

## Sprint 6: replay temporal

Estado: futuro.

- procesar secuencias y no solo imagenes;
- asociar por `track_id` cuando exista y usar `FrameLocal` de otro modo;
- aplicar TTL, decaimiento y hysteresis;
- amortiguar contradicciones aisladas;
- marcar `stale` y conservar el origen temporal de cada parte.

Puerta de salida: una fuente intermitente no cambia la postura en un solo frame
y una contradiccion persistente reduce la calidad.

## Sprint 7: adaptador de runtime

Estado: futuro y condicionado al replay.

- ensamblar la evidencia antes de `DetectionConsolidator`;
- ejecutar el analizador sin ejecutar modelos adicionales;
- publicar un resumen estrecho desde percepcion;
- decidir si el resumen llega a `mana-control` o queda en diagnostico.

Puerta de salida: el runtime conserva el contrato actual y el engine no mueve
keypoints, mascaras ni geometria interna al control.

## Puertas de decision

- No integrar Sprint 5 al FSM.
- No promover firmas con una sola captura nueva.
- No comparar modelos o ROI distintos dentro de una misma matriz.
- No usar `depth-person` contra `SurfaceCalibration`.
- No agregar una metrica que no cambie una decision, un peso o una razon.
