# 07 — Cascada con hijo de postura

## Hipótesis

La rama de pose corre bajo la misma compuerta que face en el 04: `pose-standard`
se ejecuta cuando hay exactamente una persona confirmada y visible, recorta
sobre ese track, y se saltea (y se cuenta) cuando no.

Es el primer escenario que carga el ONNX de pose —hasta ahora declarado en el
catálogo pero con `enabled = false`— y el primero que mide su costo.

## Qué está activo

|Capa|Estado|
|---|---|
|Ingesta RTSP + decode|activa|
|Inferencia|**activa** — `detect-fast` + `pose-standard` en cascada|
|Tracking|**activo** (la compuerta lo requiere)|
|Zonas / FSM|apagados|
|Presencia|apagada — el tracker no la necesita|
|Rerun|apagado|

Usa el blueprint de producción `detect-pose`. Los knobs de `[detection]`
(gobiernan la rama de face) quedan inertes y se conservan para no cambiar dos
cosas a la vez contra el 04.

## Por qué este peldaño importa

El 04 homologó la compuerta de un hijo; este homologa la **rama**. La regla es
la misma (`requires_exact_count = 1`, `requires_class = "person"`,
`requires_min_confidence = 0.50`, gate sobre el track confirmado, `same_frame =
false`), así que cualquier diferencia entre 04 y 07 pertenece a la rama de
pose: carga del modelo, latencia del modelo y consumo del lazo.

Evidencia esperada en el JSONL: detecciones de `pose-standard` con bbox,
confianza y campo `keypoints` (`[[x, y, conf], ...]` en coordenadas de frame)
cuando la compuerta abre. Los keypoints no están en coordenadas del crop:
llegan re-mapeados al frame.

## Cómo correr

```sh
timeout 180 cargo run --release -- --config workshop/scenarios/07-detect-pose/mana.toml
```

Igual que el 04, conviene correr contra las dos fuentes: `home2` (cámara real,
escena vacía) y `clip1` (RTSP local con una persona en cama). La variante de
config efectiva queda materializada junto a su salida.

## Criterios de aceptación

|Criterio|Qué se espera|
|---|---|
|La rama carga|`model pose-standard: loaded` en el arranque, sin errores de ONNX ni de task|
|La regla gobierna|`skips` de `pose-standard` **sube** sin persona y baja a 0 con una; misma razón que en el 04, distinta línea del reporte|
|El recorte sigue al track|`roi:[...]` de `pose-standard` se mueve entre ventanas|
|Los keypoints salen|Registros `detection` de `pose-standard` con campo `keypoints` en coordenadas de frame, no del crop|
|Costo medible|`evid:` **sube** respecto del 04 con la misma escena y fuente: el piso de la edad ahora incluye el costo de pose|
|Bordes|sin `kf_pisados` — el lazo sigue dando abasto con dos modelos|

**La comparación que importa es 07 contra 04.** El único cambio es el hijo; todo
lo demás es idéntico.

### Lo que sería un hallazgo

**El modelo no carga.** El ONNX de pose está en
`tools/model-tools/artifacts/yolo26-fp16/yolo26s-pose-fp16-320.onnx`; un error
de carga acá es de ruta o de task, no de compuerta.

**Skips idénticos al 04 con persona en cuadro.** Sería la compuerta muerta otra
vez — el hijo nunca corre con la escena que sí lo pide.

## Números medidos

Corridas del 2026-08-12, 180 s cada una, en release, con `YOLO26s FP16 320`
para detección y pose. El `evid` p50 es la mediana de los p50 reportados por
las ventanas de 5 s; `max` es el peor maximo reportado.

|Corrida|`evid` p50/max|`dline` p95|`pose-standard` skips|keypoints|`kf_pisados`|missed|
|---|---|---|---|---:|---:|---:|
|`clip1`, persona en cama|515 / 916 ms|1,2-1,4 ms|12 total|165|0|0|
|`home2`, fuente real|527 / 974 ms|1,8-2,0 ms|64 total|116|0|0|

`pose-standard` tuvo una mediana/maximo de inferencia de 35/42 ms en `clip1` y
36/47 ms en `home2`. Los keypoints llegaron al JSONL en coordenadas de frame y
el ROI cambio durante la corrida. No hubo `overruns`, `missed` ni
`kf_pisados`. `home2` no permanecio vacia durante toda esta ventana, por eso
sus skips no representan una escena vacia pura.
