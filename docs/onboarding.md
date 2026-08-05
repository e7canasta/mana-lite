# Mana-Lite Onboarding

Guia paso a paso para configurar el pipeline de vision y manejar la cascada de modelos.

## Indice

1. [Arquitectura de archivos](#1-arquitectura-de-archivos)
2. [Flujo del pipeline](#2-flujo-del-pipeline)
3. [El cascade: como funciona](#3-el-cascade-como-funciona)
4. [Escenarios de configuracion](#4-escenarios-de-configuracion)
5. [Depuracion y metricas](#5-depuracion-y-metricas)
6. [Checklist de puesta en marcha](#6-checklist-de-puesta-en-marcha)

---

## 1. Arquitectura de archivos

```
mana-lite/
├── mana-lite            # binario
├── config/
│   ├── mana.toml        # configuracion principal (fuente, inferencia, salud, output)
│   ├── models.toml      # catalogo de modelos (path, task, parametros)
│   ├── cascade.toml     # reglas de cascada: que modelo depende de cual
│   ├── fsm.toml         # maquina de estados (opcional — restringe modelos por estado)
│   ├── zones.toml       # zonas de interes (opcional — ROIs para FSM/tracking)
│   ├── metrics.toml     # metricas: que se loguea en texto y JSONL
│   ├── viz.toml         # visualizacion: que se envia a Rerun
│   └── rerun.toml       # blueprint: layout del dashboard en Rerun
├── models/
│   ├── yolo26n.onnx        # detect-fast: rapido, buena confianza
│   ├── yolo26s.onnx        # detect-v2: balance velocidad/precision
│   ├── yolo26x.onnx        # detect-large: lento, maxima precision
│   └── yolo26n-pose.onnx   # pose-standard: keypoints, requiere persona
├── docs/
│   ├── onboarding.md       # este archivo
│   ├── observability.md   # guia completa de metricas + viz + rerun
│   └── metrics/
│       ├── ingest-metrics.md
│       └── infer-metrics.md
└── logs/
    └── mana-YYYYMMDDTHH.jsonl
```

Cada archivo TOML tiene una responsabilidad unica:

| Archivo | Responsabilidad | Obligatorio |
|---------|----------------|-------------|
| `mana.toml` | Streaming, salud, output, paths a demas configs | Si |
| `models.toml` | Que modelos ONNX cargar y con que parametros | Si |
| `cascade.toml` | Orden y dependencias entre modelos | No (usa default) |
| `fsm.toml` | Que modelos correr en cada estado operacional | No |
| `zones.toml` | Regiones de interes para el tracker y FSM | No |
| `metrics.toml` | Que se loguea (texto + JSONL) y cada cuanto | No (usa defaults) |
| `viz.toml` | Que datos se envian a Rerun | No (usa defaults) |
| `rerun.toml` | Layout del blueprint en Rerun | No (usa default) |

---

## 2. Flujo del pipeline

```
RTSP Stream
    │
    ▼
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│  Ingest  │───▶│  Decode  │───▶│  Infer   │───▶│  Track   │
│ (retina) │    │ H264→RGB │    │ (cascade)│    │ (SORT)   │
└──────────┘    └──────────┘    └──────────┘    └──────────┘
                                      │               │
                                      ▼               ▼
                                 ┌──────────┐    ┌──────────┐
                                 │  Zones   │◀───│  Track   │
                                 │ (evaluate)    │ (active) │
                                 └──────────┘    └──────────┘
                                      │
                                      ▼
                                 ┌──────────┐
                                 │   FSM    │
                                 │ (states) │
                                 └──────────┘
                                      │
                                      ▼
                              modelos activos
                              para el proximo frame
```

**Ciclo principal** (~cada keyframe, tipicamente 0.3-1.0 Hz):

1. **Ingest**: Retina recibe frames RTP/RTSP. Filtra solo keyframes (IDR). Deduplica.
2. **Decode**: H.264 → RGB via ffmpeg (swscaler). ~10-15ms tipico.
3. **Infer**: El cascade decide que modelos correr y en que orden. Cada modelo recibe el frame RGB.
4. **Track**: SORT asigna IDs a las detecciones entre frames consecutivos.
5. **Zones**: Evalua si los tracks activos estan dentro de las ROIs definidas.
6. **FSM**: Evalua transiciones basadas en eventos de zona + health. Cambia el conjunto de modelos activos.

---

## 3. El cascade: como funciona

### Concepto

El cascade es un **grafo de dependencias entre modelos**. No es un pipeline secuencial — es un scheduler que decide, en cada frame, que modelos ejecutar basado en lo que detecto el modelo padre.

### Estructura de reglas

Cada regla tiene 3 campos:

```toml
# cascade.toml
[[rules]]
model = "pose-standard"         # este modelo...
requires = "detect-fast"        # ...solo corre SI detect-fast...
requires_class = "person"       # ...detecto al menos una "person"
```

Reglas sin `requires` son **roots** — siempre corren cuando son solicitadas:

```toml
[[rules]]
model = "detect-fast"           # root: siempre corre
```

### Orden de ejecucion

Dado el cascade actual:

```toml
[[rules]]
model = "detect-fast"           # root 1

[[rules]]
model = "detect-v2"             # root 2

[[rules]]
model = "detect-large"          # root 3

[[rules]]
model = "pose-standard"
requires = "detect-fast"
requires_class = "person"       # child
```

Si los 4 modelos estan activos (sin FSM), `ordered()` produce:

```
Frame N:
  1. detect-fast   → detecta "person" (conf 0.82), "chair" (conf 0.65)
  2. detect-v2     → detecta "person" (conf 0.74)
  3. detect-large  → detecta "person" (conf 0.91), "bed" (conf 0.70)
  4. pose-standard → CORRE: detect-fast vio "person" ✓

Frame N+1:
  1. detect-fast   → detecta "chair" (conf 0.71)    ← no "person"
  2. detect-v2     → detecta "chair" (conf 0.55)
  3. detect-large  → detecta "bed" (conf 0.77)
  4. pose-standard → SKIP: detect-fast no vio "person" ✗
```

Los roots corren primero. Los children solo si el parent detecto la clase requerida.

### Deshabilitar modelos por task

En `mana.toml`, el campo `disabled_tasks` filtra modelos completos:

```toml
[inference]
disabled_tasks = ["pose", "segment"]   # desactiva todos los modelos con task="pose" o "segment"
```

Esto filtra ANTES de pasar al cascade. Si deshabilitas `"pose"`, `pose-standard` nunca aparece en `ordered()` — el cascade ni lo evalua.

El filtro por task es complementario al cascade:
- **Cascade**: controla dependencias (pose necesita persona detectada)
- **disabled_tasks**: control de capacidad (si el HW no da, desactivas familias enteras)

---

## 4. Escenarios de configuracion

### Escenario A: Minimo — un solo modelo, sin cascade

```toml
# models.toml
[models.detect-fast]
path = "models/yolo26n.onnx"
task = "detect"
confidence = 0.5
```

```toml
# mana.toml
[inference]
model_catalog = "config/models.toml"
default_model = "detect-fast"
# sin cascade_file, sin fsm_file, sin zones_file
```

```toml
# cascade.toml (o default sin archivo)
[[rules]]
model = "detect-fast"
```

Pipeline: 1 modelo, 1 inferencia por frame. Sin overhead de cascade ni FSM.

---

### Escenario B: Cascade multi-modelo (actual)

Detecta presencia y activa pose solo cuando hay personas.

```toml
# models.toml — 4 modelos activos
[models.detect-fast]
path = "models/yolo26n.onnx"
task = "detect"
confidence = 0.5

[models.detect-v2]
path = "models/yolo26s.onnx"
task = "detect"
confidence = 0.3

[models.detect-large]
path = "models/yolo26x.onnx"
task = "detect"
confidence = 0.3

[models.pose-standard]
path = "models/yolo26n-pose.onnx"
task = "pose"
confidence = 0.3
```

```toml
# cascade.toml
[[rules]]
model = "detect-fast"

[[rules]]
model = "detect-v2"

[[rules]]
model = "detect-large"

[[rules]]
model = "pose-standard"
requires = "detect-fast"
requires_class = "person"
```

**Que esperar en Rerun:**

```
/infer/detect_fast/active/hz    = 0.5     ← siempre corre
/infer/detect_v2/active/hz      = 0.5     ← siempre corre
/infer/detect_large/active/hz   = 0.5     ← siempre corre (mas lento)
/infer/pose_standard/active/hz  = 0.3     ← solo cuando hay persona
/infer/pose_standard/warnings/skips = 1   ← frames sin persona
```

**En logs cada 5s:**

```
infer:  2.0 Hz — 10 calls in 5s | 52ms avg | 4 dets
```

Con persona: 5 frames × 4 modelos = 20 calls, pero cascade skipea pose cuando no hay persona → ~10-15 calls efectivas.

---

### Escenario C: FSM controlando modelos por estado

El FSM **restringe** que modelos se piden. Si un modelo no esta en la lista del estado actual, no se incluye en `ordered()` — ni siquiera se evalua en el cascade.

```toml
# fsm.toml
[fsm]
initial = "idle"

[fsm.states.idle]
models = ["detect-fast"]                    # solo el rapido

[fsm.states.watching]
models = ["detect-fast", "pose-standard"]   # agregamos pose

[fsm.states.alert]
models = ["detect-fast", "detect-v2", "detect-large", "pose-standard"]  # todo
```

```toml
# mana.toml
[inference]
fsm_file = "config/fsm.toml"
```

**Flujo tipico:**

```
Estado idle:
  detect-fast corre solo. Si detecta persona en zona bed → FSM transition watching.

Estado watching:
  detect-fast + pose-standard. Si persona sale de zona bed → FSM transition alert.
  Si todas las zonas vacias por 60s → FSM transition idle.

Estado alert:
  todos los modelos. Maxima precision para confirmar evento.
```

**Ventaja del FSM**: ahorra GPU/CPU en estados de baja actividad. En `idle`, solo 1 inferencia por frame en vez de 4.

```toml
# mana.toml — activar FSM
[pipeline]
fsm = true
```

---

### Escenario D: Bajo recurso (edge/Jetson)

Desactivar modelos pesados, bajar resolucion, subir intervalo de reporte.

```toml
# mana.toml
[inference]
disabled_tasks = ["pose"]       # no necesitas keypoints en edge

[health]
report_interval_s = 30          # reportar cada 30s en vez de 5s
data_stale_ms = 30000           # tolerar 30s sin frame

[pipeline]
infer = true
track = false                   # sin tracking = sin SORT overhead
zones = false
fsm = false                     # sin FSM = sin validacion de estados
```

```toml
# models.toml — solo el mas chico
[models.detect-fast]
path = "models/yolo26n.onnx"
task = "detect"
confidence = 0.4               # mas permisivo
imgsz = 416                    # resolucion reducida → mas rapido
```

---

### Escenario E: Solo grabacion (sin inferencia)

Stream + snapshots, cero GPU.

```toml
# mana.toml
[pipeline]
infer = false                   # sin inferencia
track = false

[output]
snapshot_dir = "./snapshots"
snapshot_verbose = true

[inference]
model_catalog = "config/models.toml"   # se necesita para validar config
default_model = "detect-fast"          # aunque no corre
```

Los snapshots guardan el H.264 raw y el frame RGB decodificado en `./snapshots/`.

---

## 5. Depuracion y metricas

> **Guia completa:** [docs/observability.md](observability.md) — cubre los tres archivos de configuracion (`metrics.toml`, `viz.toml`, `rerun.toml`), el arbol de entidades Rerun, diagnostico, y escenarios de uso.

### Que mirar en los logs cada 5s

```
ingest: 1.0 Hz — 5 keyframes in 5s | decode 15ms avg | cycles 80 | pframes:25 timeouts:80
infer:  2.0 Hz — 10 calls in 5s | 52ms avg | 4 dets | skips:2 empty:1
```

| Metrica | Significado | Alerta |
|---------|------------|--------|
| `ingest: X.X Hz` | Keyframes por segundo | < 0.3 → stream lento, revisar GOP |
| `decode Xms avg` | Tiempo de decode | > 100ms → CPU saturada |
| `infer: X.X Hz` | Inferencias por segundo | << ingest_hz → cuello de botella GPU |
| `Xms avg` | Latencia promedio inferencia | > 100ms → modelo muy pesado |
| `dets` | Total detecciones en la ventana | 0 persistente → camara apuntando a nada |
| `skips:N` | Modelos saltados por cascade | > 0 consistente → el parent no detecta la clase |
| `empty:N` | Inferencias con 0 detecciones | > 30% → confianza muy alta o escena vacia |
| `pframes:N` | P-frames descartados | > 100 → normal (solo procesas keyframes) |
| `timeouts:N` | Polls RTSP sin respuesta | >> ciclos → red saturada |
| `rtp:N` | Errores de paquete RTP | > 10 → probar TCP en vez de UDP |

### Rerun: arbol de metricas

Ver `docs/metrics/infer-metrics.md` para el arbol completo per-model.

**Diagnostico rapido en Rerun:**

1. Abri el panel `Infer` → mira `hz` y `avg_ms` global
2. Abri `Infer ❌` → si `skips` > 0, el cascade no esta funcionando
3. Expande `/infer/{model}/` → cada modelo tiene sus propias 3 ramas
4. Panel `Camera` → las cajas coloreadas son detecciones en vivo
5. Panel `Pipeline` → `loop_latency_us` te dice si el sistema esta saturado

### Log JSONL para analisis post-hoc

```bash
# Ver detecciones del modelo pose-standard
cat logs/mana-*.jsonl | jq 'select(.model == "pose-standard")'

# Contar detecciones por clase en la ultima hora
cat logs/mana-*.jsonl | jq -r '.det[]?.class' | sort | uniq -c | sort -rn

# Latencia promedio de inferencia
cat logs/mana-*.jsonl | jq 'select(.type == "detection") | .infer_ms' | \
  awk '{sum+=$1; n++} END {print sum/n "ms avg"}'
```

---

## 6. Checklist de puesta en marcha

- [ ] `models.toml`: solo los modelos que necesitas (comenta el resto)
- [ ] `cascade.toml`: cada child tiene `requires` y `requires_class` correctos
- [ ] `mana.toml` > `[inference]` > `cascade_file` apunta al archivo correcto
- [ ] Los modelos ONNX existen en `models/` y son compatibles con tu build de `ultralytics-inference`
- [ ] `[pipeline]` > `fsm = false` para empezar simple; activar FSM despues
- [ ] `disabled_tasks = []` o solo las tasks que queres desactivar
- [ ] Rerun viewer corriendo en `127.0.0.1:9876` si `[viz] enabled = true`
- [ ] `[health] report_interval_s = 5` para desarrollo; subir a 30-60 en produccion
- [ ] Primer run: mira el log por 30s. Confirma que `ingest: X.X Hz` coincide con el GOP de la camara
- [ ] Confirma que los modelos esperados aparecen en Rerun bajo `/infer/`
- [ ] Si `skips:N` es alto y no deberia, revisa `requires_class` en `cascade.toml`
- [ ] Si `empty:N` es alto, baja `confidence` en `models.toml` para ese modelo
