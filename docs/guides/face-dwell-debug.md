# Diagnostico de Face Dwell

El perfil `config/metrics-face-dwell-transition.toml` mantiene en el mismo
JSONL la maquina de cardinalidad de room, el snapshot de `face dwell` y las
transiciones confirmadas de la FSM facial. No reemplaza ninguna de las dos
maquinas.

Ejecutar sin cambiar el perfil operativo:

```bash
MANA_METRICS_FILE=config/metrics-face-dwell-transition.toml \
MANA_SAVE_DIR=./logs/face-dwell-transition \
MANA_JSONL_LEVEL=debug \
MANA_VIZ_ENABLED=true \
cargo run -- --config config/mana.toml
```

Reproducir esta secuencia:

1. Arrancar sin persona.
2. Entrar y permanecer fuera de `face_dwell`.
3. Mover la cara dentro de la ROI fija.
4. Sacarla de la ROI y luego salir.
5. Detener el proceso con `Ctrl-C`.

Revisar los eventos:

```bash
rg '"type":"(presence|face_dwell|fsm)"' logs/face-dwell-transition
```

En `face_dwell`:

- `state` y `state_dwell_ms` muestran el estado facial efectivo.
- `face_in_dwell` muestra la interseccion actual con la ROI fija.
- `active_timers` muestra que transicion esta acumulando dwell y cuanto falta.
- `face_was_inside` explica si `exiting` puede ser alcanzado.
- `face_model_ran` distingue ausencia de cara de un modelo no ejecutado.
- `source = "keyframe"` corresponde a una evaluacion con evidencia nueva.
- `source = "wildcard"` queda reservado para transiciones globales `from = "*"`;
  la FSM facial no debe avanzar sus estados de escena fuera del keyframe.

Comparar `presence.state` con `face_dwell.cardinality`: si divergen, el
problema esta en la entrada de una maquina, no en el nombre mostrado por
Rerun. Comparar `face_dwell.active_timers` con el evento `fsm` siguiente para
verificar que el dwell configurado se cumplio antes de la transicion.
