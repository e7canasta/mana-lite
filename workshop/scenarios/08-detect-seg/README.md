# 08 — Cascada con hijo de segmentación

## Hipótesis

La rama de segmentación corre bajo la misma compuerta que face en el 04:
`seg-standard` se ejecuta cuando hay exactamente una persona confirmada y
visible, recorta sobre ese track, y emite una máscara en el espacio del crop
con `origin` al frame que viaja al JSONL como campo `mask` (Spec-003).

Es el primer escenario que carga el ONNX de segmentación —hasta ahora con una
ruta rota y `enabled = false`— y el primero que valida la salida de máscara en
el lazo real.

## Qué está activo

|Capa|Estado|
|---|---|
|Ingesta RTSP + decode|activa|
|Inferencia|**activa** — `detect-fast` + `seg-standard` en cascada|
|Tracking|**activo** (la compuerta lo requiere)|
|Zonas / FSM|apagados|
|Presencia|apagada — el tracker no la necesita|
|Rerun|apagado|

Usa el blueprint de producción `detect-seg`. Los knobs de `[detection]`
(gobiernan la rama de face) quedan inertes y se conservan para no cambiar dos
cosas a la vez contra el 04.

## Por qué este peldaño importa

Es la primera rama cuya evidencia es **más que un bbox**: la máscara. El 08
prueba que la cadena completa funciona —ONNX, postprocess de máscara
(`mask_threshold`, componentes, RLE + polígonos) y serialización JSONL— sobre
video real, no solo en los tests unitarios de `infer`.

## Cómo correr

```sh
timeout 180 cargo run --release -- --config workshop/scenarios/08-detect-seg/mana.toml
```

Igual que el 04, conviene correr contra las dos fuentes: `home2` (cámara real,
escena vacía) y `clip1` (RTSP local con una persona en cama). La variante de
config efectiva queda materializada junto a su salida.

## Criterios de aceptación

|Criterio|Qué se espera|
|---|---|
|La rama carga|`model seg-standard: loaded` en el arranque, sin errores de ONNX ni de task|
|La regla gobierna|`skips` de `seg-standard` **sube** sin persona y baja a 0 con una|
|El recorte sigue al track|`roi:[...]` de `seg-standard` se mueve entre ventanas|
|La máscara sale|Registros `detection` de `seg-standard` con campo `mask` self-contained (`rle`, `bbox`, `origin`, `mask_dims`, `polygons`), con `origin` apuntando al frame|
|Costo medible|`evid:` **sube** respecto del 04 con la misma escena y fuente|
|Bordes|sin `kf_pisados`|

**La comparación que importa es 08 contra 04.** El único cambio es el hijo.

### Lo que sería un hallazgo

**El modelo no carga.** La ruta del baseline es
`tools/model-tools/artifacts/yolo26-fp16/yolo26s-seg-fp16-320.onnx`; los artifacts
`m/l/x` y 640 quedan reservados para las siguientes rondas de benchmark.

**Bboxes sin campo `mask`.** Sería la cadena de máscara rota aguas abajo del
modelo: postprocess o serialización, no la compuerta.

## Números medidos

Corridas del 2026-08-12, 180 s cada una, en release, con `YOLO26s FP16 320`
para detección y segmentación. El `evid` p50 es la mediana de los p50
reportados por las ventanas de 5 s; `max` es el peor maximo reportado.

|Corrida|`evid` p50/max|`dline` p95|`seg-standard` skips|detecciones con mask|`kf_pisados`|missed|
|---|---|---|---|---:|---:|---:|
|`clip1`, persona en cama|523 / 1123 ms|1,5-1,8 ms|11 total|166|0|1|
|`home2`, fuente real|482 / 1121 ms|1,9-2,1 ms|135 total|45|0|0|

`seg-standard` tuvo una mediana/maximo de inferencia de 47/54 ms en `clip1` y
47/55 ms en `home2`. La máscara sale completa en JSONL (`rle`, `origin`,
`mask_dims`, `polygons`) y la compuerta gobierna. No hubo `kf_pisados` ni
`overruns`; quedó un `missed` aislado en `clip1`, sin regresión sostenida de
cadencia. La evidencia máxima quedó cerca de 1,1 s, frente a más de 3 s con
`YOLO26x` a 640.

`home2` no permanecio vacia durante toda esta ventana, por eso sus skips no
representan una escena vacia pura.
