# 11 - Instrumento de capacidad del scheduler

## Hipotesis

La cascada completa `detect + face + pose + seg` puede medirse sin confundir
latencia del modelo, politica de intervalo y falta de target. La corrida usa una
ventana de 60 segundos porque una ventana de 5 segundos no representa bien a un
modelo configurado a `0.5 Hz`.

## Perfiles

| Perfil | Detector | Face | Pose | Seg | Blueprint |
|---|---|---|---|---|---|
| s/640 | YOLO26s 640 | YOLOv12s face 640 | YOLO26s pose 640 | YOLO26s seg 640 | `blueprint-s-640.toml` |
| m/640 | YOLO26m 640 | YOLOv12m face 640 | YOLO26m pose 640 | YOLO26m seg 640 | `blueprint-m-640.toml` |

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

Perfil `m/640`:

```sh
MANA_SOURCE_URL=rtsp://192.168.1.6:8554/clip1 \
MANA_BLUEPRINT_FILE=workshop/scenarios/11-inference-capacity/blueprint-m-640.toml \
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

## Evidencia pendiente

La corrida formal de 30 a 60 segundos por perfil requiere los pesos `s/640` y
`m/640`, además de la fuente RTSP local. Los resultados deben agregarse a este
README sin reemplazar los números de los escenarios 07-10.
