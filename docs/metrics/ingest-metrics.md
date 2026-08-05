**Arbol de metricas en Rerun**

El blueprint genera 4 paneles. Cada stage tiene su propio TimeSeriesView. Los errores de ingest van separados.

```
Blueprint layout (row shares: 5 | 1 | 1 | 1)
┌───────────────────────────────────────────┐
│  Spatial2DView "Camera"    [/world/camera]│  ██████████████████████████████  (62%)
├───────────────────────────────────────────┤
│  TimeSeries "Signals"   [/world/signals]  │  ██████  (12.5%)
├─────────────────────┬─────────────────────┤
│   TimeSeries        │    TimeSeries       │
│   "Ingest"          │    "Ingest ❌"      │  ██████  (12.5%)
│  [/ingest/normal]   │  [/ingest/errors]   │
├─────────────────────┴─────────────────────┤
│  TimeSeries "Pipeline"  [/pipeline]       │  ██████  (12.5%)
└───────────────────────────────────────────┘
```

### Arbol de paths

```
/ingest/normal/
├── hz                  ─ keyframes por segundo (promedio en ventana)
├── keyframes           ─ total de keyframes en la ventana
├── decode_avg_ms       ─ tiempo promedio de decodificacion H.264→RGB
└── pframes_dropped     ─ P-frames descartados (volumen normal)

/ingest/errors/
├── timeouts            ─ polls RTSP sin respuesta
├── ssrc_changes        ─ cambios de SSRC (stream reiniciado)
├── rtp_errors          ─ errores de paquete RTP
├── reconnect_attempts  ─ intentos de reconexion
└── dup_keyframes       ─ keyframes duplicados (camara congelada?)

/pipeline/
├── loop_latency_us     ─ latencia del superloop (per frame)
├── cycles_window       ─ vueltas del loop en la ventana
├── decode/latency_us   ─ tiempo de decode H.264→RGB (per frame)
├── infer/{model}/latency_us ─ latencia de inferencia por modelo (per frame)
├── track/total         ─ tracks totales (incluye no confirmados)
├── track/active        ─ tracks confirmados activos
├── health/ms_since_frame ─ ms desde el ultimo keyframe (per frame)
└── health/blind_cycles ─ ciclos en estado blind (per ventana)
```

### Periodo de emision

| Metrica | Frecuencia | Path |
|---------|-----------|------|
| Per-frame (decode, infer, track, health) | Cada keyframe | `/pipeline/**` |
| Per-window (ingest normal, errors) | Cada `report_interval_s` (default 5s) | `/ingest/**` |

### Interpretacion

**Ingest (normal)** — salud del stream
- `hz` ≥ 0.3 → ok. < 0.1 → stream muy lento, verificar GOP de la camara.
- `decode_avg_ms` < 30ms → ok. > 100ms → CPU saturada, considerar bajar resolucion o modelos.
- `pframes_dropped` alto → normal (el stream tiene muchos p-frames).

**Ingest ❌ (errores)** — alarmas
- `reconnect_attempts` > 2 en ventana de 5s → red inestable o camara caida.
- `dup_keyframes` persistente → escena estatica o camara congelada.
- `rtp_errors` > 10 → considerar cambiar a TCP en vez de UDP.
- `ssrc_changes` → la camara se reinicio o cambio de encoder.

**Pipeline** — diagnostico interno
- `loop_latency_us` > 100ms → el pipeline completo esta lento (inferencia pesada).
- `decode/latency_us` comparado con `infer/{model}/latency_us` → donde esta el cuello de botella.
- `health/ms_since_frame` > `data_stale_ms` → dispara `HealthTransition::Blind` → FSM → `blind`.
- `track/active` → personas/objetos en escena. 0 persistente → camara apuntando a pared.
