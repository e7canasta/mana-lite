# Observability: Metrics, Viz & Rerun Blueprint

Guia de los tres canales de salida: que va a cada uno y por que.

---

## Indice

1. [Filosofia: tres canales, tres momentos](#1-filosofia-tres-canales-tres-momentos)
2. [Text log: salud operativa](#2-text-log-salud-operativa)
3. [JSONL: registro forense](#3-jsonl-registro-forense)
4. [Rerun: tuneo en vivo](#4-rerun-tuneo-en-vivo)
5. [Arbol de entidades Rerun](#5-arbol-de-entidades-rerun)
6. [Blueprint: layout del dashboard](#6-blueprint-layout-del-dashboard)
7. [Escenarios de configuracion](#7-escenarios-de-configuracion)
8. [Referencia rapida de toggles](#8-referencia-rapida-de-toggles)

---

## 1. Filosofia: tres canales, tres momentos

Hay dos momentos distintos en la vida de este sistema:

### Momento A: Tuneando modelos (Rerun abierto)

Estas iterando. Abris Rerun, miras el dashboard. Preguntas que queres contestar:

- El modelo ve lo que espero? (boxes correctos, sin falsos positivos)
- Las detecciones son estables o flickerean? (persona 0→1→0→2 entre frames)
- La confianza es pareja o el modelo duda de algunas instancias? (conf_max - conf_min grande en un mismo frame)
- La latencia de inferencia es estable o tiene picos?
- El stream entrega frames regulares o hay gaps?

**Para esto necesitas datos per-frame, no promedios.** Un promedio de 5s te oculta que a los 2s hubo un spike de latencia o que una deteccion tenia confianza 0.3.

### Momento B: 24/7, algo fallo a las 3am

No tenes Rerun. Tenes el text log (journald) para triaje rapido y el JSONL para analisis forense. Preguntas:

- A que hora empezo el problema? → `jq 'select(.type=="frame")'` para ver gaps
- Que vio el modelo justo antes? → `jq 'select(.type=="detection" and .frame_id > 5000)'`
- Fue un problema de camara o de modelo? → timeouts vs empty detections
- El FSM transiciono correctamente? → `jq 'select(.type=="fsm")'`

### Que va a cada canal

| Canal | Frecuencia | Para que | NO para que |
|---|---|---|---|
| **Text log** (stderr) | Cada 5s | Salud operativa: Hz, latencia, flags de error | Datos per-frame (ruido) |
| **JSONL** (stdout) | Cada frame | Forense: todo lo que paso, estructurado | Monitoreo en vivo |
| **Rerun** (gRPC) | Cada frame | Tuneo: per-frame timeseries + imagen | Promedios de ventana, contadores de error |

**Regla:** si es un promedio o un contador acumulado, va en text log y JSONL. Si es un dato crudo por frame, va en Rerun y JSONL. Rerun no recibe nada que no sea per-frame.

---

## 2. Text log: salud operativa

Controlado por `config/metrics.toml` seccion `[metrics.text]`.

```toml
[metrics]
report_interval_s = 5

[metrics.text]
ingest_line = true
infer_summary = true
per_model_lines = true

[metrics.text.flags]
ingest_pframes = true
ingest_timeouts = true
ingest_reconnect = true
ingest_ssrc = true
ingest_rtp = true
infer_skips = true
infer_empty = true
```

### Salida tipica

```
ingest: 1.0 Hz — 5 keyframes in 5s | decode 15ms avg | cycles 80 | pframes:120, timeouts:80
infer:  2.0 Hz — 10 calls in 5s | 38ms avg | 2-145ms | 12 dets | skips:2, empty:1
  detect-fast:  0.4 Hz | 2 calls | 15ms avg | 10-22ms | 6/5fr
  detect-large: 0.2 Hz | 1 calls | 65ms avg | 58-145ms | 4/5fr
```

### Columnas del text log

| Columna | Significado |
|---|---|
| `X.X Hz` | Llamadas / segundos de ventana |
| `N calls` | Total de invocaciones en la ventana |
| `Xms avg` | Latencia promedio |
| `X-Xms` | Rango min-max de latencia en la ventana |
| `N/Mfr` | N detecciones totales / M frames en la ventana |
| `skip,empty` | Flags: saltos del cascade, inferencias vacias |

---

## 3. JSONL: registro forense

Controlado por `config/metrics.toml` seccion `[metrics.jsonl]`.

```toml
[metrics.jsonl]
frame_events = true              # {"type":"frame", frame_id, decode_ms, gap_ms}
detection_events = true          # {"type":"detection", model, infer_ms, det, per_class}
zone_events = true               # {"type":"zone", zone, event, class, confidence}
fsm_events = true                # {"type":"fsm", from, to, trigger, dwell_ms}
metrics_event = true             # {"type":"metrics", ...} — reporte de ventana
per_model_in_window = true       # stats por modelo en el metrics event
class_counts_in_window = true    # conteo por clase en per-model stats
class_per_frame_stats = true     # stats por clase en cada detection event
```

### Estructura de eventos

**Frame event** — uno por keyframe procesado:
```json
{"type":"frame","frame_id":10,"is_keyframe":true,"decode_ms":14,"gap_ms":6123}
```
`gap_ms` es el tiempo real entre este keyframe y el anterior. Si es > 2x el GOP esperado, el stream tuvo un gap.

**Detection event** — uno por modelo por frame, con per_class inline:
```json
{"type":"detection","frame_id":10,"model":"detect-fast","infer_ms":37,
 "det":[{"class":"person","confidence":0.73,"bbox":[875,149,1187,726]}],
 "per_class":{"person":{"count":1,"conf_min":0.73,"conf_max":0.73,
                        "area_min":228096.0,"area_max":228096.0}}}
```

### Consultas forenses con jq

```bash
# Frames donde la confianza de persona bajo de 0.5
jq 'select(.type=="detection" and .per_class.person.conf_min < 0.5)' mana-*.jsonl

# Gaps de stream > 5 segundos
jq 'select(.type=="frame" and .gap_ms > 5000)' mana-*.jsonl

# Timeline de detecciones: frame, modelo, clase, confianza
jq -c 'select(.type=="detection") | {f: .frame_id, m: .model, det: [.det[]? | {c: .class, cf: .confidence}]}' mana-*.jsonl

# Conteo de detecciones por clase en toda la sesion
jq -r 'select(.type=="metrics") | .models[].classes // {} | to_entries[] | "\(.key): \(.value)"' mana-*.jsonl | awk -F: '{a[$1]+=$2} END {for(k in a) print k, a[k]}' | sort -rnk2
```

---

## 4. Rerun: tuneo en vivo

Controlado por `config/viz.toml`. Solo datos per-frame — nada de promedios de ventana.

```toml
[viz]
enabled = true
rerun_addr = "127.0.0.1:9876"

[viz.send]
frames = true                       # RGB image per keyframe
boxes = true                        # bounding boxes
infer_latency = true                # per-model inference time per frame
decode_latency = true               # H.264 decode time per frame
class_counts_per_frame = true       # per-class count every keyframe
class_confidence_per_frame = true   # per-class conf min/max every keyframe
class_area_per_frame = true         # per-class bbox area min/max every keyframe
keyframe_gap = true                 # ms between consecutive keyframes
```

### Que ves en cada panel del blueprint

**Camera** — la imagen + los bounding boxes. Si `frames=true` y `boxes=true`. Cada clase tiene un color distinto (hash del nombre). El label muestra `"person 0.87"`.

**Counts** — cuantas detecciones de cada clase por frame. Si ves `person: 0→1→0→2→0`, el modelo esta flickereando — probablemente el threshold de confianza esta muy alto.

**Confidence** — para cada clase, `conf_min` y `conf_max` por frame. Con una deteccion, min == max. Con multiples detecciones de la misma clase, ves la dispersion: si `person/max = 0.9` y `person/min = 0.3`, el modelo esta muy seguro de una persona pero duda de otra (posible oclusion o persona parcial).

**Area** — para cada clase, `area_min` y `area_max` por frame. Si `person/area_max` crece consistentemente, la persona se esta acercando a la camara. Si todas las areas cambian simultaneamente, la camara se movio.

**Latency** — tiempo de inferencia por modelo + tiempo de decode. Si la latencia tiene picos periodicos (ej. cada 30s), puede ser thermal throttling de GPU.

**Stream** — gap en ms entre keyframes. Si es estable (ej. ~3000ms para GOP=10 a 3fps), el stream esta sano. Picos esporadicos = perdida de paquetes.

---

## 5. Arbol de entidades Rerun

Solo las entidades que Rerun recibe actualmente:

```
/world/
  camera/
    bgr                              ─ imagen RGB (Image archetype)
    detections/{model}/{class}/{i}    ─ Boxes2D con label "{class} {conf}"

/ingest/
  normal/
    gap_ms                           ─ ms entre keyframes

/pipeline/
  infer/{model}/latency_us           ─ tiempo de inferencia
  decode/latency_us                  ─ tiempo de decode

/infer/
  {model}/
    per_frame/
      counts/{class}                 ─ cuantas detecciones de esta clase este frame
      conf/{class}/min               ─ confianza minima de esta clase este frame
      conf/{class}/max               ─ confianza maxima de esta clase este frame
      area/{class}/min               ─ area minima de esta clase este frame
      area/{class}/max               ─ area maxima de esta clase este frame
```

---

## 6. Blueprint: layout del dashboard

Tres filas. Nada mas.

```
┌─────────────────────────────────────────────┐
│ Camera (spatial2d)              share 5     │
├──────────┬──────────┬───────────────────────┤
│ Counts   │Confidence│ Area                  │  share 1
├──────────┴──────────┴───────────────────────┤
│ Latency              │ Stream               │  share 1
└──────────────────────┴──────────────────────┘
```

El blueprint se envia por codigo al conectar (`send_default_blueprint`). `config/rerun.toml` es una referencia declarativa del mismo layout para importacion manual.

---

## 7. Escenarios de configuracion

### Tuneo de modelo (Rerun abierto)

```toml
# viz.toml — todo encendido
[viz.send]
frames = true
boxes = true
infer_latency = true
decode_latency = true
class_counts_per_frame = true
class_confidence_per_frame = true
class_area_per_frame = true
keyframe_gap = true
```

```toml
# metrics.toml — JSONL con per_class para consultas post-hoc
[metrics.jsonl]
detection_events = true
class_per_frame_stats = true
```

### Produccion 24/7 (sin Rerun)

```toml
# viz.toml
[viz]
enabled = false
```

```toml
# metrics.toml — text log minimal, JSONL completo para forense
[metrics.text]
ingest_line = true
infer_summary = true
per_model_lines = false

[metrics.jsonl]
frame_events = true
detection_events = true
zone_events = true
fsm_events = true
metrics_event = true
class_per_frame_stats = true
```

### Edge / bajo recurso

```toml
# viz.toml
[viz]
enabled = false

# metrics.toml
[metrics]
report_interval_s = 60

[metrics.text]
ingest_line = true
infer_summary = false
per_model_lines = false

[metrics.jsonl]
detection_events = true
# el resto false
```

---

## 8. Referencia rapida de toggles

### viz.toml (8 toggles)

| Toggle | Default | Rerun entity |
|---|---|---|
| `frames` | true | `/world/camera/bgr` |
| `boxes` | true | `/world/camera/detections/{model}/{class}/{i}` |
| `infer_latency` | true | `/pipeline/infer/{model}/latency_us` |
| `decode_latency` | true | `/pipeline/decode/latency_us` |
| `class_counts_per_frame` | true | `/infer/{model}/per_frame/counts/{class}` |
| `class_confidence_per_frame` | true | `/infer/{model}/per_frame/conf/{class}/{min,max}` |
| `class_area_per_frame` | true | `/infer/{model}/per_frame/area/{class}/{min,max}` |
| `keyframe_gap` | true | `/ingest/normal/gap_ms` |

### metrics.toml — text

| Toggle | Default | Que imprime |
|---|---|---|
| `ingest_line` | true | `ingest: X.X Hz — N keyframes in Ns...` |
| `infer_summary` | true | `infer:  X.X Hz — N calls in Ns...` |
| `per_model_lines` | true | `  detect-fast: X.X Hz \| N calls \| Xms...` |

### metrics.toml — jsonl

| Toggle | Default | Evento |
|---|---|---|
| `frame_events` | true | frame_id, decode_ms, **gap_ms** |
| `detection_events` | true | model, infer_ms, det[], **per_class{}** |
| `zone_events` | true | zone, event, class, confidence |
| `fsm_events` | true | from, to, trigger, dwell_ms |
| `metrics_event` | true | window aggregates (5s) |
| `per_model_in_window` | true | per-model stats dentro de metrics_event |
| `class_counts_in_window` | true | class_counts dentro de per-model |
| `class_per_frame_stats` | true | per_class dentro de detection_event |
