# Observability: Metrics, Viz & Rerun Blueprint

Guia de los tres canales de salida: que va a cada uno y por que.

Para procedimientos de administracion y troubleshooting paso a paso, ver
[operations.md](operations.md).

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

**Regla:** si es un promedio o un contador acumulado, va en text log y JSONL. Si es un dato crudo por frame, va en Rerun y JSONL. Rerun no recibe nada que no sea per-frame. Las tasas de ingest se calculan por muestra usando el intervalo entre selecciones de keyframe.

### Unidades Y Normalizacion

`source_hz`, `processed_hz` y `gap_ms` no deben competir en el mismo eje:
representan unidades distintas. El valor absoluto de `gap_ms` se conserva para
troubleshooting, pero el dashboard puede usar estas derivaciones para una
escala comun:

```text
expected_period_ms = 1000 / source_hz
gap_ratio          = gap_ms / expected_period_ms
drop_ratio         = dropped / seen
throughput_ratio   = min(1.0, processed_hz / source_hz)
freshness          = min(1.0, expected_period_ms / gap_ms)
```

`drop_ratio`, `throughput_ratio` y `freshness` deben representarse
entre `0` y `1`. `gap_ratio` no se acota: `1.0` significa un periodo normal y
`2.0` significa que transcurrieron dos periodos esperados. Para comparar
graficamente, usar las metricas acotadas; para investigar tiempos, usar
`gap_ms` y `gap_ratio` en un panel separado.

Si `source_hz`, `seen` o `gap_ms` son cero, omitir la muestra derivada en vez
de fabricar un cero que parezca una degradacion.

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
ingest_keyframe_drops = true
ingest_timeouts = true
ingest_reconnect = true
ingest_ssrc = true
ingest_rtp = true
infer_skips = true
infer_empty = true
```

### Salida tipica

```
ingest: 1.0 Hz — 5 keyframes processed (5 seen) in 5s | decode 15ms avg | cycles 80 | pframes:120, timeouts:80
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
detection_events = true          # {"type":"detection", model, infer_ms, pipeline_ms, det, per_class}
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
`gap_ms` es el tiempo entre este keyframe procesado y el anterior. Si hubo
drops, no debe interpretarse por si solo como un gap del stream; usar
`source_hz` y `dropped` en Rerun.

El evento `metrics` tambien incluye `keyframes_seen` y `keyframes_dropped` para
el analisis forense de latest-frame-wins.

**Detection event** — uno por modelo por frame, con per_class inline:
```json
{"type":"detection","frame_id":10,"model":"detect-fast","infer_ms":37,"pipeline_ms":42,
 "det":[{"class":"person","confidence":0.73,"bbox":[875,149,1187,726]}],
 "per_class":{"person":{"count":1,"conf_min":0.73,"conf_max":0.73,
                         "area_min":228096.0,"area_max":228096.0}}}
```

En el experimento de cascada, `face-yolo` solo debe aparecer en JSONL y Rerun
cuando el frame padre tiene exactamente una `person`. Su bbox es evidencia
secundaria de esa persona, no una observacion primaria equivalente. Una face
sin persona compatible queda solo en el `detection` crudo y no entra en
`consolidated_detection`.

En Rerun, las detecciones crudas se publican bajo
`/world/camera/detections/<model>`. Para `face-yolo`, revisar
`/world/camera/detections/face-yolo`; las cajas rojas son las caras detectadas
por el modelo, mientras que `/world/camera/observations` muestra la observacion
consolidada de la persona.

El `iou` del catalogo y el `postprocess.nms_iou` son controles distintos: el
primero pertenece al NMS del backend y el segundo al NMS explicito posterior de
mana-lite. Despues de ese NMS se puede aplicar `postprocess.max_detections`,
que conserva el top-K por confianza.

**Consolidated detection event** — una observacion por sujeto espacial del
frame, sin memoria ni `track_id`:

```json
{"type":"consolidated_detection","frame_id":10,"class":"person",
 "confidence":0.73,"bbox":[875,149,1187,726],
 "primary_model":"detect-fast","sources":["detect-fast"]}
```

### Consultas forenses con jq

```bash
# Frames donde la confianza de persona bajo de 0.5
jq 'select(.type=="detection" and .per_class.person.conf_min < 0.5)' mana-*.jsonl

# Gaps de stream > 5 segundos
jq 'select(.type=="frame" and .gap_ms > 5000)' mana-*.jsonl

# Timeline de detecciones: frame, modelo, clase, confianza
jq -c 'select(.type=="detection") | {f: .frame_id, m: .model, det: [.det[]? | {c: .class, cf: .confidence}]}' mana-*.jsonl

# Timeline de consolidacion stateless
jq -c 'select(.type=="consolidated_detection") | {f: .frame_id, c: .class, cf: .confidence, bb: .bbox, src: .sources}' mana-*.jsonl

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
infer_rate = true                   # per-model call frequency
decode_latency = true               # H.264 decode time per frame
class_counts_per_frame = true       # per-class count every keyframe
class_confidence_per_frame = true   # per-class conf min/max every keyframe
class_area_per_frame = true         # per-class bbox area min/max every keyframe
keyframe_gap = true                 # ms between consecutive keyframes
keyframe_rate = true                # source_hz and processed_hz
keyframe_drops = true               # keyframes replaced by a fresher one
```

### Que ves en cada panel del blueprint

**Camera** — la imagen + las observaciones consolidadas. Si `frames=true` y `boxes=true`. En modo stateless las boxes viven bajo `/world/camera/observations`, no tienen `track_id` y el label muestra clase, confianza y modelo primario.

**Counts** — cuantas detecciones de cada clase por frame. Si ves `person: 0→1→0→2→0`, el modelo esta flickereando — probablemente el threshold de confianza esta muy alto.

**Confidence** — para cada clase, `conf_min` y `conf_max` por frame. Con una deteccion, min == max. Con multiples detecciones de la misma clase, ves la dispersion: si `person/max = 0.9` y `person/min = 0.3`, el modelo esta muy seguro de una persona pero duda de otra (posible oclusion o persona parcial).

**Area** — para cada clase, `area_min` y `area_max` por frame. Si `person/area_max` crece consistentemente, la persona se esta acercando a la camara. Si todas las areas cambian simultaneamente, la camara se movio.

**Latency** — `latency_us` es el tiempo reportado por el backend; `pipeline_us`
es el tiempo wall-clock del modelo completo, incluyendo preparacion, backend y
postprocess. Para explicar drops, usar `pipeline_us`. Si la latencia tiene
picos periodicos (ej. cada 30s), puede ser thermal throttling de GPU.

`/pipeline/infer/<model>/hz` es la frecuencia real de llamadas del modelo en
Rerun. En la configuracion actual debe aparecer `detect-fast`; `face-yolo`
aparecera solo en frames donde la cascada encuentre exactamente una persona.

**Stream** — `source_hz` estima los keyframes vistos por el demuxer, mientras
`processed_hz` muestra los que llegaron a procesamiento. `dropped` confirma
que latest-frame-wins descarta keyframes viejos cuando la inferencia no termina
a tiempo. `gap_ms` es el intervalo entre keyframes procesados, no una medicion
pura del stream cuando hubo drops. Para una lectura de salud comun, usar
`drop_ratio`, `throughput_ratio` y `freshness`.

---

## 5. Arbol de entidades Rerun

Solo las entidades que Rerun recibe actualmente:

```
/world/
  camera/
    bgr                              ─ imagen RGB (Image archetype)
    observations/{i}                  ─ Boxes2D consolidadas, frame-locales
    entities/{track_id}               ─ Boxes2D trackeadas, solo con tracking

/ingest/
  normal/
    gap_ms                           ─ ms entre keyframes
  keyframes/
    source_hz                        ─ keyframes vistos
    processed_hz                     ─ keyframes procesados
    dropped                          ─ keyframes reemplazados

/pipeline/
  infer/{model}/latency_us           ─ tiempo de inferencia
  infer/{model}/pipeline_us          ─ tiempo wall-clock del modelo
  infer/{model}/hz                   ─ frecuencia de llamadas del modelo
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

Dentro de `Stream`, mantener los valores crudos (`source_hz`, `processed_hz`,
`gap_ms`) separados de las derivaciones normalizadas (`drop_ratio`,
`throughput_ratio`, `freshness`). No superponer milisegundos y Hz en el mismo
eje.

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
keyframe_rate = true
keyframe_drops = true
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

### viz.toml (13 toggles)

| Toggle | Default | Rerun entity |
|---|---|---|
| `frames` | true | `/world/camera/bgr` |
| `boxes` | true | `/world/camera/observations/{i}`; entities si tracking |
| `crop_frames` | true | `/world/camera/crops/{model}/bgr` |
| `roi_rects` | true | `/world/camera/rois/{model}/roi/0` |
| `infer_latency` | true | `/pipeline/infer/{model}/{latency_us,pipeline_us}` |
| `infer_rate` | true | `/pipeline/infer/{model}/hz` |
| `decode_latency` | true | `/pipeline/decode/latency_us` |
| `class_counts_per_frame` | true | `/infer/{model}/per_frame/counts/{class}` |
| `class_confidence_per_frame` | true | `/infer/{model}/per_frame/conf/{class}/{min,max}` |
| `class_area_per_frame` | true | `/infer/{model}/per_frame/area/{class}/{min,max}` |
| `keyframe_gap` | true | `/ingest/normal/gap_ms` |
| `keyframe_rate` | true | `/ingest/keyframes/{source_hz,processed_hz}` |
| `keyframe_drops` | true | `/ingest/keyframes/dropped` |

### metrics.toml — text

Los defaults del schema son `true`, pero el archivo operativo puede apagarlos.
Los valores de `config/metrics.toml` son los que gobiernan la instancia.

| Toggle | Default | Que imprime |
|---|---|---|
| `ingest_line` | true | `ingest: X.X Hz — N keyframes processed (M seen)...` |
| `infer_summary` | true | `infer:  X.X Hz — N calls in Ns...` |
| `per_model_lines` | true | `  detect-fast: X.X Hz \| N calls \| Xms...` |

### metrics.toml — jsonl

Los eventos opcionales se filtran antes de entrar al buffer JSONL. Los eventos
`detection` y `consolidated_detection` comparten `detection_events`.

| Toggle | Default | Evento |
|---|---|---|
| `frame_events` | true | frame_id, decode_ms, **gap_ms** |
| `detection_events` | true | model, infer_ms, pipeline_ms, det[], **per_class{}** |
| `consolidated_detection` | `jsonl_level=debug` | frame, class, bbox, primary_model, sources |
| `zone_events` | true | zone, event, class, confidence |
| `fsm_events` | true | from, to, trigger, dwell_ms |
| `metrics_event` | true | window aggregates (5s) |
| `per_model_in_window` | true | per-model stats dentro de metrics_event |
| `class_counts_in_window` | true | class_counts dentro de per-model |
| `class_per_frame_stats` | true | per_class dentro de detection_event |
