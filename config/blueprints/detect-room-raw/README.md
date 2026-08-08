# `detect-room-raw`

Perfil temporal de calibracion del detector primario. No carga children y no
requiere tracking, por lo que permite observar `empty`, `single` y `multiple`
desde el conteo raw y sus filtros temporales.

Para usarlo durante una prueba:

```toml
[inference]
blueprint_file = "config/blueprints/detect-room-raw/blueprint.toml"

[pipeline]
track = false

[presence.occupancy]
require_confirmed_tracks = false
```

Este perfil no sirve para validar continuidad de `track_id`, face, pose,
segmentacion ni reglas clinicas de cama. Para eso se debe volver al blueprint
24/7 con tracking.
