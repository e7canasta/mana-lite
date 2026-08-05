# Observability: Metrics, Viz & Rerun Blueprint

Guia completa para configurar que se mide, como se visualiza y como se estructura el dashboard en Rerun.

---

## Indice

1. [Los tres archivos TOML](#1-los-tres-archivos-toml)
2. [Conceptos: per-frame vs per-window](#2-conceptos-per-frame-vs-per-window)
3. [Text log: metricas operativas cada N segundos](#3-text-log-metricas-operativas-cada-n-segundos)
4. [JSONL: registro forense por frame](#4-jsonl-registro-forense-por-frame)
5. [Rerun: visualizacion en tiempo real](#5-rerun-visualizacion-en-tiempo-real)
6. [Arbol de entidades Rerun](#6-arbol-de-entidades-rerun)
7. [Blueprint: layout del dashboard](#7-blueprint-layout-del-dashboard)
8. [Escenarios de configuracion](#8-escenarios-de-configuracion)
9. [Diagnostico y troubleshooting](#9-diagnostico-y-troubleshooting)
10. [Referencia rapida de toggles](#10-referencia-rapida-de-toggles)

---

## 1. Los tres archivos TOML

```
config/
├── metrics.toml     ─ que se escribe en el log de texto y JSONL
├── viz.toml         ─ que se envia a Rerun por gRPC
└── rerun.toml       ─ como se organizan los paneles en el viewer
```

Cada archivo controla un **canal de salida** distinto. Se referencian desde `mana.toml`:

```toml
# mana.toml
metrics_file = "config/metrics.toml"    # opcional: usa defaults si no existe
viz_file = "config/viz.toml"            # opcional: usa defaults si no existe
rerun_file = "config/rerun.toml"        # opcional: blueprint de referencia
```

### Separacion de responsabilidades

| Archivo | Controla | Destino | Frecuencia |
|---------|---------|---------|------------|
| `metrics.toml` > `text` | Lineas de log informativas | stderr / journald | Cada `report_interval_s` (default 5s) |
| `metrics.toml` > `jsonl` | Eventos estructurados por frame | stdout (JSONL rotativo) | Cada keyframe |
| `viz.toml` > `send` | Escalares, imagenes, cajas | Rerun viewer (gRPC) | Cada keyframe + cada ventana |
| `rerun.toml` > `rows` | Layout de paneles del viewer | Rerun blueprint | Al conectar |

**Regla de oro**: el text log es para operadores (salud del sistema). El JSONL es para analisis post-hoc (forense). Rerun es para debug en vivo. No mezclar promedios engañosos en el text log.

---

## 2. Conceptos: per-frame vs per-window

Mana-Lite emite dos tipos de datos:

### Per-frame (cada keyframe)

Datos crudos, sin promediar, una muestra por frame procesado.

| Que se emite | Donde |
|---|---|
| Latencia de inferencia por modelo (us) | `/pipeline/infer/{model}/latency_us` |
| Latencia de decode (us) | `/pipeline/decode/latency_us` |
| Conteo de detecciones por clase | `/infer/{model}/per_frame/counts/{class}` |
| Confianza min/max por clase | `/infer/{model}/per_frame/conf/{class}/{min,max}` |
| Area min/max de bbox por clase | `/infer/{model}/per_frame/area/{class}/{min,max}` |
| Gap entre keyframes (ms) | `/ingest/normal/gap_ms` |
| Imagen RGB + bounding boxes | `/world/camera/**` |

### Per-window (cada `report_interval_s`, default 5s)

Agregados, promedios y contadores acumulados en la ventana.

| Que se emite | Donde |
|---|---|
| Hz de ingesta e inferencia | `/ingest/normal/hz`, `/infer/active/hz` |
| Latencia promedio, min, max (ms) | `/infer/active/{avg_ms,min_ms,max_ms}` |
| Conteo de detecciones acumulado por clase | `/infer/{model}/classes/{class}` |
| Confianza y area promedio (ventana) | `/infer/{model}/detections/{conf_avg,conf_min,area_avg}` |
| Skips, empties, errores de red | `/infer/{model}/warnings/*`, `/ingest/errors/*` |

### Por que dos frecuencias

- **Per-frame** → ves estabilidad del tiempo de inferencia, cambios bruscos en confianza, gaps de continuidad, spikes/silencios de detecciones.
- **Per-window** → ves throughput sostenido (Hz), tendencias de calidad, salud de la red.

Si solo miras promedios cada 5s, perdes la variabilidad frame a frame. Los per-frame scalars te dan la granularidad para detectar oscilaciones.

---

## 3. Text log: metricas operativas cada N segundos

Controlado por `config/metrics.toml` seccion `[metrics.text]`.

```toml
[metrics]
report_interval_s = 5            # cada cuanto se imprime el reporte

[metrics.text]
ingest_line = true               # linea resumen de ingesta
infer_summary = true             # linea resumen de inferencia
per_model_lines = true           # una linea por modelo

[metrics.text.flags]
ingest_pframes = true            # mostrar contador de p-frames descartados
ingest_dup = true                # mostrar contador de keyframes duplicados
ingest_timeouts = true           # mostrar timeouts de poll
ingest_reconnect = true          # mostrar intentos de reconexion
ingest_ssrc = true               # mostrar cambios de SSRC
ingest_rtp = true                # mostrar errores de paquete RTP
infer_skips = true               # mostrar modelos saltados por cascade
infer_empty = true               # mostrar inferencias con 0 detecciones
```

### Salida tipica

```
ingest: 1.0 Hz — 5 keyframes in 5s | decode 15ms avg | cycles 80 | pframes:120, timeouts:80
infer:  2.0 Hz — 10 calls in 5s | 38ms avg | 2-145ms | 12 dets | skips:2, empty:1
  detect-fast:  0.4 Hz | 2 calls | 15ms avg | 10-22ms | 6/5fr | empty:1
  detect-large: 0.2 Hz | 1 calls | 65ms avg | 58-145ms | 4/5fr
  pose-standard: 0.0 Hz | 0 calls | ---ms | --- | 0/5fr | skip:empty
```

### Que significan las columnas

| Columna | Significado |
|---|---|
| `X.X Hz` | Llamadas / segundos de ventana |
| `N calls` | Total de invocaciones en la ventana |
| `Xms avg` | Latencia promedio |
| `X-Xms` | Rango min-max de latencia en la ventana |
| `N/Mfr` | N detecciones totales / M frames en la ventana |
| `skip,empty` | Flags: saltos del cascade, inferencias vacias |

### Cuando desactivar lineas

```toml
# Produccion silenciosa — solo errores
[metrics.text]
ingest_line = false
infer_summary = false
per_model_lines = false
```

El texto es para operadores humanos. En produccion automatizada, el JSONL es suficiente.

---

## 4. JSONL: registro forense por frame

Controlado por `config/metrics.toml` seccion `[metrics.jsonl]`. Cada tipo de evento se emite (o no) por frame.

```toml
[metrics.jsonl]
frame_events = true              # {"type":"frame", frame_id, decode_ms}
detection_events = true          # {"type":"detection", model, infer_ms, det:[...]}
zone_events = true               # {"type":"zone", zone, event, class, confidence}
fsm_events = true                # {"type":"fsm", from, to, trigger, dwell_ms}
metrics_event = true             # {"type":"metrics", ...}  — reporte de ventana
per_model_in_window = true       # incluir stats por modelo en el metrics event
class_counts_in_window = true    # incluir conteo por clase en el per-model stats
class_per_frame_stats = true     # incluir stats por clase en el detection event
```

### Estructura de un evento "detection" con class_per_frame_stats

```json
{
  "type": "detection",
  "frame_id": 42,
  "model": "detect-fast",
  "infer_ms": 38,
  "det": [
    {"class": "person", "confidence": 0.87, "bbox": [0.2, 0.3, 0.5, 0.8]}
  ],
  "per_class": {
    "person": {"count": 1, "conf_min": 0.87, "conf_max": 0.87, "area_min": 86400.0, "area_max": 86400.0}
  }
}
```

### Cuando desactivar eventos

```toml
# Solo eventos de FSM y zonas (caso clinico: tracking de transiciones)
[metrics.jsonl]
frame_events = false
detection_events = false
zone_events = true
fsm_events = true
metrics_event = false
```

### Analisis post-hoc con jq

```bash
# Ver la serie temporal de confianza por modelo
cat mana-*.jsonl | jq -c 'select(.type == "detection") | {frame: .frame_id, model, conf: [.det[].confidence]}'

# Extraer conteo por clase por frame
cat mana-*.jsonl | jq -c 'select(.type == "detection") | {frame: .frame_id, model, counts: .per_class}'

# Buscar gaps de > 500ms entre keyframes (si incluimos gap en frame event)
cat mana-*.jsonl | jq 'select(.type == "frame") | .frame_id' | awk 'NR>1{print $1-prev} {prev=$1}'
```

---

## 5. Rerun: visualizacion en tiempo real

Controlado por `config/viz.toml` seccion `[viz.send]`. Cada toggle habilita o corta un canal de datos hacia el viewer Rerun.

```toml
[viz]
enabled = true                           # encendido/apagado global
rerun_addr = "127.0.0.1:9876"            # donde esta corriendo rerun

[viz.send]
# ── Per-frame (un scalar por keyframe) ──
frames = true                            # RGB image (pesado ~2-6 MB/frame)
boxes = true                             # bounding boxes con labels y colores
decode_latency = true                    # tiempo de decode H.264 → RGB
infer_latency = true                     # latencia de inferencia por modelo
class_counts_per_frame = true            # conteo de detecciones por clase, por frame
class_confidence_per_frame = true        # confianza min/max por clase, por frame
class_area_per_frame = true              # area min/max de bbox por clase, por frame
keyframe_gap = true                      # ms entre keyframes consecutivos

# ── Per-frame auxiliares ──
frame_id = true                          # contador de frame (escalonado)
loop_latency = true                      # wall-clock del superloop
track_counts = true                      # tracks totales + activos
health_ms = true                         # ms desde el ultimo frame

# ── Per-window (un scalar por report_interval_s) ──
model_window_metrics = true              # hz, avg/min/max ms, yield por modelo
class_counts = true                      # conteo por clase acumulado en la ventana
ingest_window = true                     # hz de ingesta, decode_avg, errores
infer_window = true                      # hz de inferencia global, avg/min/max
pipeline_window = true                   # ciclos, blind cycles
```

### Que desactivar segun el uso

**Debug de inferencia** (minimo para ver latencia y gaps):
```toml
frames = false          # sin imagen → banda minima
boxes = false           # sin cajas
decode_latency = false
infer_latency = true    # solo latencia de inferencia
class_counts_per_frame = true
class_confidence_per_frame = true
class_area_per_frame = false
keyframe_gap = true     # continuidad del stream
frame_id = true
loop_latency = false
model_window_metrics = true
ingest_window = false
infer_window = false
pipeline_window = false
```

**Debug visual** (maximo para ver la escena):
```toml
frames = true           # imagen + cajas
boxes = true
decode_latency = false
infer_latency = false
class_counts_per_frame = true    # para ver si el modelo "ve" lo esperado
keyframe_gap = false
frame_id = false
model_window_metrics = false
```

**Produccion** (solo lo esencial para monitoreo):
```toml
frames = false
boxes = false
decode_latency = false
infer_latency = false
class_counts_per_frame = true   # anomalias de deteccion
keyframe_gap = true             # gaps de stream
frame_id = false
model_window_metrics = true     # throughput sostenido
class_counts = true             # acumulado por clase
ingest_window = true            # salud del ingest
infer_window = true
pipeline_window = false
```

---

## 6. Arbol de entidades Rerun

Las entidades se organizan en jerarquias con prefijos funcionales. Rerun las muestra automaticamente en el panel Streams agrupadas por prefijo.

### Jerarquia completa

```
/world/
  camera/
    bgr                          ─ imagen RGB (archetype Image)
    detections/{model}/{class}/{i} ─ Boxes2D con label "{class} {conf}"

/ingest/
  normal/
    hz                           ─ keyframes/segundo (ventana)
    keyframes                    ─ total en ventana
    decode_avg_ms                ─ decode promedio en ventana
    pframes_dropped              ─ p-frames descartados en ventana
    gap_ms                       ─ ms entre keyframes (per-frame)
    instant_hz                   ─ Hz instantaneo (1/dt) si se emite
  errors/
    timeouts                     ─ polls sin respuesta
    ssrc_changes                 ─ cambios de fuente RTP
    rtp_errors                   ─ paquetes corruptos
    reconnect_attempts           ─ reconexiones
    dup_keyframes                ─ keyframes duplicados

/infer/
  {model}/
    active/
      hz                         ─ inferencias/segundo (ventana)
      avg_ms                     ─ latencia promedio (ventana)
      min_ms                     ─ latencia minima (ventana)
      max_ms                     ─ latencia maxima (ventana)
      yield_avg                  ─ detecciones por inferencia (ventana)
    warnings/
      skips                      ─ saltos del cascade (ventana)
      empty                      ─ inferencias sin detecciones (ventana)
    detections/
      conf_avg                   ─ confianza promedio (ventana)
      conf_min                   ─ confianza minima (ventana)
      area_avg                   ─ area promedio (ventana)
    classes/{class}              ─ conteo acumulado por clase (ventana)
    per_frame/                   ★ NUEVO: per-frame, no promediado
      counts/{class}             ─ cuantas detecciones esta clase en este frame
      conf/{class}/min           ─ confianza minima de esta clase en este frame
      conf/{class}/max           ─ confianza maxima de esta clase en este frame
      area/{class}/min           ─ area minima de esta clase en este frame
      area/{class}/max           ─ area maxima de esta clase en este frame

/pipeline/
  infer/{model}/latency_us       ─ latencia de inferencia (per-frame)
  decode/latency_us              ─ latencia de decode (per-frame)
  loop_latency_us                ─ superloop wall-clock (per-frame)
  track/total                    ─ total de tracks
  track/active                   ─ tracks activos
  cycles_window                  ─ ciclos del superloop (ventana)
  health/
    ms_since_frame               ─ ms desde ultimo frame
    blind_cycles                 ─ ciclos sin frame (ventana)
```

### Convenciones de nombres de entidad

- Los nombres de modelo usan `_` como separador: `detect-fast` → `detect_fast`
- Las clases usan `_` para espacios y caracteres especiales: `"dining table"` → `dining_table`
- Los paths nunca tienen caracteres fuera de `[a-zA-Z0-9_/-]`
- Las clases aparecen dinamicamente — no hay que pre-registrarlas

---

## 7. Blueprint: layout del dashboard

El blueprint define como Rerun organiza los paneles al conectar. Hay dos vias:

### Via A: Blueprint enviado por codigo (actual)

`VizBridge::send_default_blueprint()` envia un layout predefinido al conectar:

```
┌─────────────────────────────────┐
│ Camera (spatial2d)     share 5  │  ← imagen + bounding boxes
├──────────┬──────────┬───────────┤
│ Counts   │Confidence│ Area      │  ← per-frame per-class (timeseries)
├──────────┴──────────┴───────────┤
│ Latency           │ Signals     │  ← infer/decode latencies + gap + frame_id
├──────────┴──────────┴───────────┤
│              Timeline            │
└──────────────────────────────────┘
```

- **Camera**: `/world/camera/**` — muestra la imagen y los bboxes superpuestos
- **Counts**: `/infer/**/per_frame/counts/**` — conteo por clase, cada frame
- **Confidence**: `/infer/**/per_frame/conf/**` — conf min/max por clase
- **Area**: `/infer/**/per_frame/area/**` — area min/max por clase
- **Latency**: `/pipeline/infer/**/latency_us` + `/pipeline/decode/latency_us`
- **Signals**: `/ingest/normal/gap_ms` + `/world/signals/frame_id`

### Via B: Blueprint declarativo (config/rerun.toml)

Estructura de referencia que refleja la Via A. El viewer puede cargarlo manualmente (File > Import Blueprint) o se puede implementar un builder automatico en el futuro.

```toml
[rerun]
app = "mana-lite"
auto_views = false                  # no generar vistas automaticas
panels_expanded = true              # paneles de navegacion abiertos

[[rerun.rows]]
kind = "spatial2d"
name = "Camera"
origin = "/world/camera"
share = 5.0                         # 5 partes de altura

[[rerun.rows]]
kind = "horizontal"
name = "Per-Frame Classes"
share = 1.0
panels = [
    { kind = "timeseries", name = "Counts",     origin = "/infer", contents = ["+ /infer/**/per_frame/counts/**"] },
    { kind = "timeseries", name = "Confidence", origin = "/infer", contents = ["+ /infer/**/per_frame/conf/**"] },
    { kind = "timeseries", name = "Area",       origin = "/infer", contents = ["+ /infer/**/per_frame/area/**"] },
]

[[rerun.rows]]
kind = "horizontal"
name = "Latency & Signals"
share = 1.0
panels = [
    { kind = "timeseries", name = "Latency", origin = "/pipeline",     contents = ["+ $origin/infer/**/latency_us", "+ $origin/decode/latency_us"] },
    { kind = "timeseries", name = "Signals", origin = "/ingest/normal", contents = ["+ /ingest/normal/gap_ms", "+ /world/signals/frame_id"] },
]
```

### Personalizar el blueprint

Agrega o quita paneles editando `send_default_blueprint()` en `src/viz.rs`. Cada panel Rerun se construye con:

```rust
rerun::blueprint::TimeSeriesView::new("Nombre")
    .with_origin("/prefijo")
    .with_contents(["+ /prefijo/**"])   // filtro de entidades
```

---

## 8. Escenarios de configuracion

### Escenario A: Desarrollo — maxima visibilidad

Queres ver todo. Rerun abierto, log verboso, JSONL completo.

```toml
# metrics.toml
[metrics]
report_interval_s = 5

[metrics.text]
ingest_line = true
infer_summary = true
per_model_lines = true
[metrics.text.flags]
ingest_pframes = true
ingest_dup = true
ingest_timeouts = true
ingest_reconnect = true
ingest_ssrc = true
ingest_rtp = true
infer_skips = true
infer_empty = true

[metrics.jsonl]
frame_events = true
detection_events = true
zone_events = true
fsm_events = true
metrics_event = true
per_model_in_window = true
class_counts_in_window = true
class_per_frame_stats = true

# viz.toml — todo encendido
[viz.send]
frames = true
boxes = true
# ... todos true

# rerun.toml — tres filas: camara, clases, latencias
```

### Escenario B: Produccion — solo anomalias

Queres saber si algo falla. Minimo ancho de banda a Rerun.

```toml
# metrics.toml
[metrics]
report_interval_s = 30

[metrics.text]
ingest_line = true          # solo linea de ingesta
infer_summary = false
per_model_lines = false
[metrics.text.flags]
# solo flags que indican problemas
ingest_timeouts = true
ingest_reconnect = true
infer_skips = true
infer_empty = true
# el resto false

[metrics.jsonl]
frame_events = false
detection_events = true     # forense: guardar detecciones
zone_events = true          # forense: guardar cambios de zona
fsm_events = true           # forense: guardar transiciones
metrics_event = false
class_per_frame_stats = false   # no necesario en produccion

# viz.toml — minimal
[viz.send]
frames = false
boxes = false
class_counts_per_frame = true   # solo conteo para detectar anomalias
keyframe_gap = true             # gaps de stream
model_window_metrics = true     # throughput
ingest_window = true
infer_window = true
# el resto false
```

### Escenario C: Laboratorio — tuning de modelo

Queres calibrar confianza y ver precision del modelo.

```toml
# viz.toml — enfoque en calidad de deteccion
[viz.send]
frames = true                          # ver la imagen
boxes = true                           # ver las cajas
infer_latency = true                   # estabilidad del modelo
class_counts_per_frame = true          # cuantas detecciones por clase
class_confidence_per_frame = true      # confianza min/max por clase ← clave
class_area_per_frame = true            # tamanio de bbox por clase
keyframe_gap = false
model_window_metrics = true            # promedios de ventana
class_counts = true
# resto false

[metrics.jsonl]
detection_events = true
class_per_frame_stats = true           # guardar per_class en JSONL
```

### Escenario D: Edge/Jetson — minimo overhead

```toml
# viz.toml
[viz]
enabled = false                        # sin Rerun

# metrics.toml
[metrics]
report_interval_s = 60

[metrics.text]
ingest_line = true
infer_summary = false
per_model_lines = false

[metrics.jsonl]
detection_events = true                # solo lo esencial
class_per_frame_stats = false
```

---

## 9. Diagnostico y troubleshooting

### No veo datos en el panel "Counts" de Rerun

1. Verifica que `class_counts_per_frame = true` en `viz.toml`
2. Confirma que el modelo esta produciendo detecciones (mira `infer_summary` en el log)
3. Las entidades de clase aparecen dinamicamente — necesitas al menos un frame con detecciones
4. Revisa el panel Streams de Rerun: busca `/infer/{model}/per_frame/counts/`

### Las graficas de confianza min/max son lineas rectas

Si conf_min == conf_max para una clase, es porque solo hay una deteccion de esa clase por frame. Normal en escenas con pocos objetos. Cuando hay multiples instancias de la misma clase (ej. 3 personas), veras separacion entre min y max.

### El gap entre keyframes es irregular

- `gap_ms` > 1000ms consistente → la camara tiene GOP grande o hay perdida de paquetes
- Picos esporadicos de > 2000ms → reconexion RTSP o saturacion de red
- `gap_ms` = 0 intermitente → keyframes duplicados (revisa `ingest_dup_keyframes`)

### Los bounding boxes no coinciden con la imagen

- El modelo usa coordenadas normalizadas [0,1] que se convierten a pixeles con el tamano del frame decodificado
- Si la imagen se ve estirada en Rerun, es un bug de aspecto — Rerun deberia respetar el aspect ratio nativo

### Rerun no se conecta

Mana-Lite usa backoff exponencial (1s → 2s → 4s → ... → 30s max). Abri el viewer primero, luego lanza mana-lite. El log mostrara:

```
viz: will connect to rerun at 127.0.0.1:9876 when viewer opens
viz: connected to 127.0.0.1:9876
```

Si ves `connect failed`, el viewer no esta escuchando en ese puerto.

---

## 10. Referencia rapida de toggles

### viz.toml

| Toggle | Default | Tipo | Que emite |
|---|---|---|---|
| `frames` | true | per-frame | Imagen RGB |
| `boxes` | true | per-frame | Bounding boxes |
| `decode_latency` | true | per-frame | `/pipeline/decode/latency_us` |
| `infer_latency` | true | per-frame | `/pipeline/infer/{model}/latency_us` |
| `class_counts_per_frame` | true | per-frame | `/infer/{model}/per_frame/counts/{class}` |
| `class_confidence_per_frame` | true | per-frame | `/infer/{model}/per_frame/conf/{class}/{min,max}` |
| `class_area_per_frame` | true | per-frame | `/infer/{model}/per_frame/area/{class}/{min,max}` |
| `keyframe_gap` | true | per-frame | `/ingest/normal/gap_ms` |
| `frame_id` | true | per-frame | `/world/signals/frame_id` |
| `loop_latency` | true | per-frame | `/pipeline/loop_latency_us` |
| `track_counts` | true | per-frame | `/pipeline/track/{total,active}` |
| `health_ms` | true | per-frame | `/pipeline/health/ms_since_frame` |
| `model_window_metrics` | true | per-window | `/infer/{model}/active/{hz,avg_ms,...}` |
| `class_counts` | true | per-window | `/infer/{model}/classes/{class}` |
| `ingest_window` | true | per-window | `/ingest/normal/{hz,keyframes,...}` |
| `infer_window` | true | per-window | `/infer/active/{hz,avg_ms,...}` |
| `pipeline_window` | true | per-window | `/pipeline/{cycles_window,blind_cycles}` |

### metrics.toml — text

| Toggle | Default | Que imprime |
|---|---|---|
| `ingest_line` | true | `ingest: X.X Hz — N keyframes in Ns...` |
| `infer_summary` | true | `infer:  X.X Hz — N calls in Ns...` |
| `per_model_lines` | true | `  detect-fast: X.X Hz \| N calls \| Xms...` |
| `flags.ingest_pframes` | true | `\| pframes:N` |
| `flags.ingest_dup` | true | `\| dup:N` |
| `flags.ingest_timeouts` | true | `\| timeouts:N` |
| `flags.ingest_reconnect` | true | `\| reconnect:N` |
| `flags.ingest_ssrc` | true | `\| ssrc:N` |
| `flags.ingest_rtp` | true | `\| rtp:N` |
| `flags.infer_skips` | true | `\| skips:N` |
| `flags.infer_empty` | true | `\| empty:N` |

### metrics.toml — jsonl

| Toggle | Default | Evento JSONL |
|---|---|---|
| `frame_events` | true | `{"type":"frame", frame_id, decode_ms}` |
| `detection_events` | true | `{"type":"detection", model, infer_ms, det, per_class}` |
| `zone_events` | true | `{"type":"zone", zone, event, class, confidence}` |
| `fsm_events` | true | `{"type":"fsm", from, to, trigger, dwell_ms}` |
| `metrics_event` | true | `{"type":"metrics", window_s, ...}` |
| `per_model_in_window` | true | `model_metrics` dentro de metrics_event |
| `class_counts_in_window` | true | `class_counts` dentro de per-model stats |
| `class_per_frame_stats` | true | `per_class` dentro de detection_events |
