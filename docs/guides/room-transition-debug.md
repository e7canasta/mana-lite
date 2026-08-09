# Diagnostico de Transiciones de Room

El pipeline publica muchos eventos, pero para depurar `empty -> single` solo
necesitamos una linea JSONL por evaluacion valida. El perfil
`config/metrics-room-transition.toml` deja activo el evento `presence` y
desactiva detecciones, depth, zonas, FSM y metricas de ventana.

Ejecutar sin cambiar el perfil operativo:

```bash
MANA_METRICS_FILE=config/metrics-room-transition.toml \
MANA_SAVE_DIR=./logs/room-transition-timed \
MANA_JSONL_LEVEL=debug \
MANA_VIZ_ENABLED=true \
cargo run -- --config config/mana.toml
```

Reproducir esta secuencia:

1. Arrancar sin persona en el ROI.
2. Entrar y permanecer frente a la camara.
3. Salir y permanecer fuera.
4. Detener el proceso con `Ctrl-C`.

Cada evento `presence` contiene la evidencia de las dos maquinas:

- `keyframe_gap_ms`: tiempo real desde el keyframe procesado anterior.
- `source_window_ms`: ventana de keyframes observada por ingest.
- `keyframes_seen`: keyframes recibidos desde ingest.
- `keyframes_dropped`: keyframes reemplazados por latest-frame-wins.
- `state`: cardinalidad de room (`empty`, `single`, `multiple`).
- `poi_state`: presencia (`absent`, `present`, `ambiguous`).
- `raw_count`: personas raw del detector primario.
- `signal_valid`: si la inferencia primaria produjo salida valida.
- `poi_positive_ms`: milisegundos validos acumulados del filtro base de presencia.
- `poi_empty_ms`: milisegundos reales acumulados del filtro base de ausencia.
- `single_timer_ms`: tiempo del TON `empty -> single`.
- `empty_timer_ms`: tiempo del TOF `single -> empty`.
- `multiple_candidate_timer_ms`: tiempo del TON de `multiple`.
- `multiple_exit_timer_ms`: tiempo del TOF de salida de `multiple`.
- `held`: si el POI sostuvo una observacion durante un dropout.

La secuencia esperada con `on_ms = 200` es:

```text
raw_count=0, signal_valid=true  -> state=empty
raw_count=1, poi_state=present  -> state=empty, single_timer_ms < 3000
raw_count=1 sustained          -> state=single, single_timer_ms >= 3000
raw_count=0                    -> state=single, empty_timer_ms < 8000
raw_count=0                    -> state=empty,  empty_timer_ms >= 8000
```

Si `raw_count=1` pero `poi_state=absent`, el problema esta en el TON de
presencia o en su entrada. Si `poi_state=present` pero `state=empty`, el
problema esta en la maquina de cardinalidad. Si `raw_count` permanece en cero,
el problema esta antes de ambas maquinas, en deteccion o consolidacion.

Para revisar solo los eventos relevantes:

```bash
rg '"type":"presence"' logs/room-transition-timed
```
