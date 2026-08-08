# `detect-room-face`

Perfil de cardinalidad raw temporizada con enriquecimiento facial y una FSM de
ciclo de vida para una sola persona. Usa tracking para estabilizar las zonas y
publica entidades con `track_id`.

Cuando el estado de la habitacion es `single`, `face-yolo` analiza un crop de
deteccion dinamico derivado del track confirmado de la persona. El catalogo lo
configura como un cuadrado de 320 px centrado en la mitad superior del bbox de
la persona:

```text
detect-fast: [420, 0, 1500, 1080]
face-yolo:   crop dinamico de person (320x320, upper_fraction=0.50)
```

El crop de deteccion facial es independiente del ROI fijo del padre y puede
extenderse fuera de el. Las detecciones se mantienen en coordenadas del frame
original. La FSM usa ademas una ROI fija de dwell de 400x300 en el centro
superior (`[760, 0, 1160, 300]` para 1920x1080); no es el crop de inferencia.

Estados:

- `empty`: no ejecuta face.
- `single`: ejecuta face sobre el crop dinamico de la persona y avanza la FSM facial.
- `multiple`: no ejecuta face.

Estados de la FSM facial:

- `idle`: sin sesion facial activa.
- `searching`: una persona confirmada, buscando cara.
- `detected`: cara confirmada fuera de la ROI fija de dwell y del borde.
- `other`: persona presente sin cara visible luego del tiempo de busqueda.
- `in_bed`: cara dentro de la ROI fija `face_dwell`.
- `edge`: persona proxima al borde del ROI de deteccion del padre.
- `exiting`: ausencia sostenida despues de haber estado dentro.

`exiting` requiere el latch interno `face_was_inside`; una persona que nunca
fue confirmada como `detected` o `in_bed` vuelve a `idle` sin generar salida.

La cardinalidad usa los mismos timers monotónicos que el perfil raw:

```toml
[presence.occupancy]
single_confirm_ms = 3000
empty_confirm_ms = 8000
multiple_confirm_ms = 5000
multiple_exit_ms = 5000
require_confirmed_tracks = false
```

El estado se publica en Rerun por frame y en JSONL cuando se habilita
`face_dwell_events`. En Rerun, `log_time` es el tiempo de envío y `frame_time`
es el timestamp compartido por estado, frame BGR y crops. Para una prueba
enfocada en transiciones se puede seleccionar:

```toml
metrics_file = "config/metrics-room-transition.toml"
```

Para analizar la cardinalidad y el dwell facial en la misma secuencia:

```toml
metrics_file = "config/metrics-face-dwell-transition.toml"
```

Ese perfil conserva `presence_events`, activa `face_dwell_events` y activa
`fsm_events`. El evento `face_dwell` registra el estado actual, el tiempo en
ese estado, el progreso de los timers candidatos y la evidencia de cara; el
evento `fsm` conserva la transicion confirmada.

Para activarlo:

```toml
[inference]
blueprint_file = "config/blueprints/detect-room-face/blueprint.toml"

[pipeline]
track = true
zones = true
fsm = true
```

La FSM se carga desde `config/blueprints/detect-room-face/fsm.toml` y las zonas
desde `config/zones.toml`.

El ROI y sus detecciones se publican en Rerun bajo:

- `/world/camera/rois/face-yolo`
- `/world/camera/rois/fixed/detect-fast`
- `/world/camera/rois/fixed/face-dwell/roi`
- `/world/camera/crops/face-yolo/bgr`
- `/world/camera/crops/face-yolo/detections`
- `/world/camera/detections/face-yolo`
- `/pipeline/state/room/cardinality`
- `/pipeline/state/room/second_person`
- `/pipeline/state/room/signal`

`face-yolo` solo se ejecuta cuando la cardinalidad ya esta en `single`; el crop
dinamico usa el track confirmado del padre durante un dropout corto del
detector. Su imagen y sus boxes se publican con el `frame_nr` y `frame_time`
actuales. La ocupacion de persona sigue usando `zones.bed`; la FSM facial usa
`face_dwell` y ambas regiones son independientes del crop de `face-yolo`.
