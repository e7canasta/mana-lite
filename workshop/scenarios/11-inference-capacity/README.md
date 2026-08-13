# 11 - Instrumento de capacidad del scheduler

## Hipotesis

La cascada completa `detect + face + pose + seg` puede medirse sin confundir
latencia del modelo, politica de intervalo y falta de target. La corrida usa una
ventana de 60 segundos porque una ventana de 5 segundos no representa bien a un
modelo configurado a `0.5 Hz`.

## Perfiles

| Perfil | Detector | Face | Pose | Seg | Blueprint |
|---|---|---|---|---|---|
| s/192 | YOLO26s 192 | YOLOv12s face 192 | YOLO26s pose 192 | YOLO26s seg 192 | `blueprint-s-192.toml` |
| m/192 | YOLO26m 192 | YOLOv12m face 192 | YOLO26m pose 192 | YOLO26m seg 192 | `blueprint-m-192.toml` |
| s/320 | YOLO26s 320 | YOLOv12s face 320 | YOLO26s pose 320 | YOLO26s seg 320 | `blueprint-s-320.toml` |
| m/320 | YOLO26m 320 | YOLOv12m face 320 | YOLO26m pose 320 | YOLO26m seg 320 | `blueprint-m-320.toml` |

Los overlays cambian solamente path y `imgsz`; las reglas de cascade y los
umbrales permanecen iguales. En ambos perfiles, `detect`, `face` y `pose` tienen
intervalo `0` y `seg` tiene `interval_min_ms = 2000` para ejercitar la política
de `0.5 Hz` dentro de la ventana larga.

## Como correr

Desde la raiz, con una fuente que contenga una persona:

```sh
MANA_SOURCE_URL=rtsp://192.168.1.6:8554/clip1 \
  cargo run --release -- --config workshop/scenarios/11-inference-capacity/mana.toml
```

Perfil `s/192`:

```sh
MANA_SOURCE_URL=rtsp://192.168.1.6:8554/clip1 \
  MANA_BLUEPRINT_FILE=workshop/scenarios/11-inference-capacity/blueprint-s-192.toml \
  cargo run --release -- --config workshop/scenarios/11-inference-capacity/mana.toml
```

La salida larga debe contener un evento `metrics` cada 60 segundos y una linea
por modelo con:

```text
interval ... | gap n:... p50:... p95:... max:... | due_late n:... p50:... p95:... max:...
```

## Criterios de aceptacion

- Los cuatro modelos cargan y aparecen en el reporte de ventana.
- `interval_min_ms` aparece como `0` en detect/face/pose y `2000` en seg.
- `gap_*` se calcula entre inicios del mismo modelo, no entre resultados.
- `due_late_*` queda sin muestras para el primer inicio y muestra atraso en los siguientes si el modelo empieza tarde.
- `not_due` sube para seg entre inicios; `due_but_no_target` refleja una escena sin target.
- `due_but_gated` sólo sube cuando el estado no solicita un modelo que ya estaba debido.
- `urgent` y `urgent_expired` permanecen en cero hasta Sprint 3.
- `kf_pisados`, `img_pisadas` y `dline missed` siguen siendo las compuertas de salud del pipeline.

## Evidencia medida

Corridas del 2026-08-12, 60 segundos por perfil, `--release`, contra
`rtsp://192.168.1.6:8554/clip1`. Los cuatro modelos cargaron correctamente y
la ventana mantuvo el lazo en 5 Hz.

|Perfil|`dline` p95/max|Inferencias|Detect ms|Face ms|Pose ms|Seg ms|`kf_dropped`|missed|
|---|---:|---:|---:|---:|---:|---:|---:|---:|
|`s/192`|2.0/2.1 ms|74|18-27|17-24|16-19|25-30|0|0|
|`s/320`|1.2/1.8 ms|177|33-41|36-47|33-47|52-64|0|0|
|`m/192`|1.9/2.0 ms|189|40-49|39-49|38-46|57-66|0|0|
|`m/320`|1.4/4.2 ms|179|87-106|90-113|87-105|137-156|1|0|

`s/192` reduce el costo, pero en esta escena produjo sólo 6 llamadas de
`face-yolo` y `pose-standard`, frente a 49 en `s/320`; debe tratarse como un
perfil de frecuencia, no como sustituto clínico automático. `s/320` es el
perfil recomendado por defecto. `m/320` queda fuera de la recomendacion CPU por
el costo y el keyframe descartado. Los perfiles `640` existentes se conservan
sólo como referencia histórica.

## A/B de calidad

`clip1` es un MP4 que `go2rtc` publica en loop. Cada corrida abre un nuevo
consumidor RTSP y vuelve a reproducir la misma secuencia; no hace falta cambiar
ni reiniciar `go2rtc` entre perfiles. La medicion se hizo en corridas
secuenciales de 60 segundos con los blueprints existentes:

```sh
MANA_SOURCE_URL=rtsp://127.0.0.1:8554/clip1 \
MANA_BLUEPRINT_FILE=workshop/scenarios/11-inference-capacity/blueprint-s-320.toml \
MANA_METRICS_FILE=workshop/scenarios/11-inference-capacity/metrics-quality.toml \
MANA_JSONL_LEVEL=debug \
MANA_SAVE_DIR=/tmp/mana-quality/s320 \
cargo run --release -- --config workshop/scenarios/11-inference-capacity/mana.toml
```

Para `m/192` se cambia solamente el blueprint y el directorio de salida.
`metrics-quality.toml` habilita eventos por modelo con bbox, confianza,
keypoints y RLE de mascara. El crop de `face-yolo` queda en ambos casos
derivado del bbox de persona, cuadrado de 320 px y limitado a la mitad
superior.

Resultados preliminares de la primera ventana de 60 segundos:

| Tarea | s/320 | m/192 | Lectura provisional |
|---|---|---|---|
| Detect | 58/59 frames con persona, confianza media 0.904 | 58/59, 0.878 | s tiene mas confianza; m rechazo menos candidatos y su bbox fue algo mas estable |
| Face | 49/49 caras, media 0.717; 42 >= 0.60 | 54/54 caras, media 0.776; 54 >= 0.60 | m/192 es claramente mejor candidato |
| Pose | 49/49 poses; 47 con >=13 keypoints validos | 52/54 poses; 52 con >=13 keypoints validos | m/192 aporta mas cobertura de keypoints |
| Seg | 20/20 mascaras, confianza media 0.926 | 21/21 mascaras, confianza media 0.876 | s tiene mayor confianza; m tiene una ocupacion/estabilidad levemente mayor |

Ambos perfiles mantuvieron 61 keyframes, cero `kf_dropped`, cero `dline
missed` y cero reconexiones. Esto todavia no es precision estadistica: sin ground
truth anotado mide confianza, cobertura y estabilidad. La decision provisional es
`m/192` para face y pose, `s/320` o `m/192` segun si detect prioriza precision o
recall, y una inspeccion visual/ground truth pendiente para segmentacion.
