# `detect-room-face`

Perfil de calibracion de cardinalidad raw temporizada con enriquecimiento facial
para una sola persona. No requiere tracking ni publica `track_id`.

Cuando el estado de la habitacion es `single`, `face-yolo` recibe un ROI
dinamico de la mitad superior del bbox de la persona. Las detecciones del crop
se trasladan automaticamente a las coordenadas del frame original.

Estados:

- `empty`: no ejecuta face.
- `single`: ejecuta face sobre el ROI dinamico.
- `multiple`: no ejecuta face.

La cardinalidad usa los mismos timers monotónicos que el perfil raw:

```toml
[presence.occupancy]
single_confirm_ms = 3000
empty_confirm_ms = 8000
multiple_confirm_ms = 5000
multiple_exit_ms = 5000
require_confirmed_tracks = false
```

El estado se publica en JSONL y Rerun por frame. En Rerun, `log_time` es el
tiempo de envío y `frame_time` es el timestamp compartido por estado, frame BGR
y crops. Para una prueba enfocada en transiciones se puede seleccionar:

```toml
metrics_file = "config/metrics-room-transition.toml"
```

Para activarlo:

```toml
[inference]
blueprint_file = "config/blueprints/detect-room-face/blueprint.toml"

[pipeline]
track = false
```

El ROI y sus detecciones se publican en Rerun bajo:

- `/world/camera/rois/face-yolo`
- `/world/camera/crops/face-yolo/detections`
- `/world/camera/detections/face-yolo`
- `/pipeline/state/room/cardinality`
- `/pipeline/state/room/second_person`
- `/pipeline/state/room/signal`

`face-yolo` solo se ejecuta cuando la cardinalidad ya está en `single`; sus
datos se sincronizan con el mismo `frame_nr` y `frame_time` del frame padre.
