# Mana-Lite Onboarding

Guia paso a paso para configurar el pipeline de vision y manejar la cascada de modelos.

Para operacion diaria, configuracion administrativa, lectura de logs y Rerun,
ver [operations.md](operations.md). Este documento se concentra en el modelo
mental de ingenieria y en el flujo del codigo.

El blueprint y contrato operativo completo de depth esta en
[specs/depth-standard.md](specs/depth-standard.md). Las especificaciones de
mascaras y de la rama de segmentacion estan en [specs/](specs/).

## Indice

1. [Arquitectura de archivos](#1-arquitectura-de-archivos)
2. [Flujo del pipeline](#2-flujo-del-pipeline)
3. [El cascade: como funciona](#3-el-cascade-como-funciona)
4. [ROI — crops por modelo](#4-roi--crops-por-modelo)
5. [Escenarios de configuracion](#5-escenarios-de-configuracion)
6. [Depuracion y metricas](#6-depuracion-y-metricas)
7. [Checklist de puesta en marcha](#7-checklist-de-puesta-en-marcha)
8. [Depth standard](specs/depth-standard.md)

---

## 1. Arquitectura de archivos

```
mana-lite/
├── mana-lite            # binario
├── config/
│   ├── mana.toml        # configuracion principal (fuente, inferencia, salud, output)
│   ├── models.toml      # manifest publico del catalogo
│   ├── models/           # base, defaults, perfiles y un archivo por task
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
│   ├── yolo26n-pose.onnx   # pose-standard: keypoints, requiere persona
│   ├── yolov12l-face.onnx  # face-yolo: face sobre persona (same_frame)
│   └── yolo26*-seg/depth   # seg-standard / depth-standard (FP16)
├── docs/
│   ├── onboarding.md       # este archivo
│   ├── operations.md       # guia para administradores y operadores
│   ├── observability.md    # guia completa de metricas + viz + rerun
│   ├── roi.md              # crops por modelo — static y dinamico
│   ├── ARCHITECTURE.md     # modulos, ownership y ciclo principal
│   ├── ROADMAP.md          # estado actual y proximas etapas
│   ├── adrs/               # decisiones de diseno (001-024)
│   └── specs/              # blueprints y contratos (depth, seg, mask)
└── logs/
    └── mana-YYYYMMDDTHH.jsonl
```

Cada archivo TOML tiene una responsabilidad unica:

| Archivo | Responsabilidad | Obligatorio |
|---------|----------------|-------------|
| `mana.toml` | Streaming, salud, output, paths a demas configs | Si |
| `models.toml` | Manifest publico de modelos y archivos incluidos | Si |
| `models/*.toml` | Defaults, perfiles y overrides por task | Si |
| `blueprints/<name>/blueprint.toml` | Perfil seleccionado: modelos activos, root y gates | Recomendado |
| `blueprints/<name>/models.toml` | Overrides opcionales del catálogo para ese blueprint | Opcional |
| `[presence.poi]` en `mana.toml` | Histeresis de la persona de interes | Recomendado en 24/7 |
| `[presence.occupancy]` en `mana.toml` | Confirmacion de empty/single/multiple | Recomendado en calibracion |
| `cascade.toml` | Orden y dependencias entre modelos | No (usa default) |
| `fsm.toml` | Que modelos correr en cada estado operacional | No |
| `zones.toml` | Regiones de interes para tracks y FSM | No |
| `metrics.toml` | Que se loguea (texto + JSONL) y cada cuanto | No (usa defaults) |
| `viz.toml` | Que datos se envian a Rerun | No (usa defaults) |
| `rerun.toml` | Layout del blueprint en Rerun | No (usa default) |

---

## 2. Flujo del pipeline

```
RTSP Stream
    │
    ▼
┌──────────┐    ┌──────────┐    ┌────────────────────────┐
│  Ingest  │───▶│  Decode  │───▶│  Infer (cascade)       │
│ (retina) │    │ H264→RGB │    │  detect-fast (root)    │
└──────────┘    └──────────┘    │   ├── pose-standard    │
                                │   ├── face-yolo        │
                                │   └── seg-standard     │
                                │  depth-standard (root) │ ← ROI local
                                └────────────────────────┘
                                        │
                                        ▼
                                   ┌──────────┐
                                   │Consolid. │  (depth NO entra)
                                   │(stateless)│
                                   └──────────┘
                                        │
                                        ▼
                                    ┌──────────┐       tracking=true
                                    │  Track   │──────────────┐
                                    │ optional │              │
                                    └──────────┘              ▼
                                    ┌──────────┐
                                    │Occupancy │  empty/single/multiple
                                    │  state   │  policy separada
                                    └──────────┘
                                         │
                                         ▼
                                    ┌──────────┐    ┌──────────┐
                                   │  Zones   │◀───│  Entity  │
                                   │ (optional)    │ (tracked) │
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
3. **Infer**: El cascade decide que modelos correr y en que orden. Cada modelo recibe el frame RGB (o su crop). `depth-standard` recibe el crop de su ROI fijo y produce el mapa depth local; no depende de detecciones.
4. **Consolidate**: `Detection` de cada modelo se fusiona, sin memoria temporal, en `ConsolidatedObservation` y sus evidencias se asocian por relación espacial. Depth no entra aquí.
5. **POI signal**: `presence.poi` adquiere y sostiene la persona de interes antes del tracker.
6. **Track (opcional)**: con `pipeline.track = true`, el tracker asigna IDs y mantiene `TrackedEntity` entre frames.
7. **Occupancy state**: la maquina clasifica `empty`, `single` o `multiple` con timers monotono de room; la segunda persona puede requerir tracks confirmados segun la politica.
8. **Publish observations**: JSONL emite detecciones y presencia; Rerun dibuja `/world/camera/observations`, entidades y la timeline `/pipeline/state/room`.
9. **Zones/FSM (opcionales)**: consumen tracks y siguen representando eventos clinicos de cama, separados de cardinalidad.

---

## 3. El cascade: como funciona

El perfil activo se elige en `mana.toml` con `inference.blueprint_file`. Los
blueprints recomendados son `detect-face` para calibracion y
`detect-face-pose-seg` para 24/7 con tracking. La especificacion completa esta
en [specs/inference-blueprints.md](specs/inference-blueprints.md).

### Concepto

El cascade es un **grafo de dependencias entre modelos**. No es un pipeline secuencial — es un scheduler que decide, en cada frame, que modelos ejecutar basado en la evidencia temporal y espacial del modelo padre.

El modelo root produce detecciones. Después de NMS y filtros básicos, el
`DetectionConsolidator` fusiona las salidas del ciclo. Si el tracking está
habilitado, el tracker confirma la identidad temporal y los modelos hijos solo
se habilitan con tracks confirmados y visibles; una detección aislada no dispara
pose.

Los filtros básicos viven en `[models.<name>.postprocess]` y son propios de
cada modelo: las detecciones que no pasan clase permitida, confianza, área o
validación geométrica se eliminan antes de metrics, Rerun, JSONL y consolidación.
Las reglas de cascade no vuelven a filtrar la salida publicada; solo aplican
condiciones adicionales para habilitar hijos.

Las regiones semánticas de la cascada son distintas de los crops físicos y de
las zonas clínicas. La región responde a "¿esta persona pertenece a la escena
relevante?"; el crop responde a "¿qué píxeles debe procesar el siguiente
modelo?".

### Estructura de reglas

Cada regla tiene dependencias, filtros opcionales y scope:

```toml
# cascade.toml
[regions.bed]
rect = [100, 200, 500, 800]

[[rules]]
model = "pose-standard"         # este modelo...
requires = "detect-fast"        # ...solo corre SI detect-fast...
requires_class = "person"       # ...detecto al menos una "person"
```

Para una cascada que depende de la escena actual, una regla puede usar:

```toml
[[rules]]
model = "face-yolo"
requires = "detect-fast"
requires_class = "person"
requires_exact_count = 1       # solo exactamente una persona
same_frame = true              # usa la deteccion del parent del mismo frame
```

`same_frame = true` no necesita un track confirmado. Es apropiado para un
modelo secundario como face que debe correr sobre el bbox de la deteccion
actual, no sobre una identidad temporal.

Reglas sin `requires` son **roots** — siempre corren cuando son solicitadas:

```toml
[[rules]]
model = "detect-fast"           # root: siempre corre
```

### Orden de ejecucion

Dado el cascade actual (`config/cascade.toml`):

```toml
[[rules]]
model = "detect-fast"           # root 1

[[rules]]
model = "depth-standard"        # root 2 — independiente

[[rules]]
model = "pose-standard"
requires = "detect-fast"
requires_class = "person"       # child
requires_min_confidence = 0.50
requires_min_area_ratio = 0.01
requires_region = "bed"
requires_region_coverage = 0.30
same_frame = true

[[rules]]
model = "face-yolo"
requires = "detect-fast"
requires_class = "person"
requires_exact_count = 1        # solo exactamente una persona
same_frame = true

[[rules]]
model = "seg-standard"
requires = "detect-fast"
requires_class = "person"
same_frame = true
```

Los roots corren primero, en orden declarado. Los children solo si el parent
detecto la clase requerida. `depth-standard` es un root deliberado: corre
aunque no haya detecciones, no entra en consolidación y sus estadisticas se
validan con `valid_pixels`, no con detecciones.

### Consolidación de entidades

Una detección es la salida de un modelo, no una entidad clínica:

```text
detect-fast:   person bbox P
pose:          person bbox P' + keypoints
face:          face bbox F
                         │
                         ▼
TrackedEntity 7: person bbox P + pose evidence + face component
```

El detector primario aporta el bbox canónico. Pose, face y segment enriquecen
el track. Face se asocia por containment, no por IoU puro. Una silla de ruedas
no se fusiona con la persona automáticamente: son entidades distintas.

Si un modelo secundario corre a menor frecuencia, su evidencia conserva un
`last_seen` propio y puede incorporarse al mismo track cuando llegue.

### Deshabilitar modelos (flag `enabled`)

En `models.toml`, el campo `enabled` filtra un modelo completo antes del
cascade (ADR-020):

```toml
[models.seg-standard]
enabled = false     # desactiva la rama: no se carga ni se infiere
```

Esto filtra ANTES de pasar al cascade. Si deshabilitas `seg-standard`, nunca
aparece en `ordered()` — el cascade ni lo evalua. Es el mecanismo actual para
controlar coste por rama (sustituye al viejo `disabled_tasks`, que operaba por
`task` y podía apagar la raíz).

El toggle es complementario al cascade:
- **Cascade**: controla dependencias (pose necesita persona detectada)
- **`enabled`**: control de capacidad (si el HW no da, desactivas ramas enteras)

---

## 4. ROI — crops por modelo

> **Guia completa:** [docs/roi.md](roi.md) — las dos fuentes, las tres politicas (`min_region`, `max_region`, `fallback`), recetas practicas, y verificacion detallada.

### Concepto

Cada modelo puede recortar el frame antes de inferir. El modelo solo ve los pixeles de una region → procesa menos, se distrae menos.

| Motivacion | Sin ROI | Con ROI |
|---|---|---|
| **Velocidad** | 640x480 → toda la escena | 400x300 → 2.5x menos pixeles |
| **Precision** | Fondo agrega ruido al clasificador | Solo la zona relevante |
| **Privacidad** | Detecta objetos en la puerta | `max_region` excluye la puerta |

Un crop de la mitad del area reduce ~4x los pixeles que el modelo procesa.

---

### Las dos fuentes

#### Static — rectangulo fijo

Para camaras fijas que miran un area predecible. Coordenadas en pixeles del frame original.

```toml
[models.detect-fast.crop]
type = "static"
region = [120, 90, 520, 390]  # x1, y1, x2, y2 — centro del frame
```

El ROI se calcula una vez al cargar el modelo. No depende de detecciones. El modelo siempre corre en esa region.

**Ideal para:** camara apuntando a una cama, silla, puerta — zonas fijas.

#### LargestClass — dinamico desde el parent

El ROI se recalcula cada frame tomando el bbox mas grande de una clase detectada por el modelo padre.

```toml
[models.pose-standard.crop]
type = "largest_class"
class = "person"
margin = 0.15                    # expande el bbox 15% en cada direccion
```

**Ideal para:** modelo hijo que solo debe mirar donde el padre encontro algo. Ej: pose solo donde hay persona, face solo donde hay cabeza.

---

### Pipeline mental

```
Frame 640x480
    │
    ▼
┌─────────────────────────────┐
│  Modelo padre (detect-fast) │  ← frame completo, sin crop
│  detecta: person @ [150,100,350,400], chair @ [500,200,600,300]
└─────────────────────────────┘
    │
    ▼
┌─────────────────────────────┐
│  resolve_crop_rect()        │
│  largest_class("person")    │
│  → bbox[150,100→350,400]    │
│  + margin 15%               │
│  → crop [120,70,380,430]    │
└─────────────────────────────┘
    │
    ▼  crop_rect = {x1:120, y1:70, x2:380, y2:430}
┌─────────────────────────────┐
│  Modelo hijo (pose)         │
│  recorta RGB a 260x360      │
│  infiere → keypoints         │
│  +offset (120,70) → frame   │  ← coordenadas corregidas
└─────────────────────────────┘
    │
    ▼  detecciones en espacio original
Tracking, zonas, FSM, JSONL — sin cambios
```

---

### Las tres politicas (solo `largest_class`)

```toml
[models.bed-detector.crop]
type = "largest_class"
class = "person"
margin = 0.15
min_region = [100, 200, 500, 450]   # piso: nunca mas chico
max_region = [0, 0, 640, 400]       # techo: nunca mas grande
fallback = "full"                    # "skip" (default) o "full"
```

| Politica | Funcion | Ejemplo |
|---|---|---|
| `min_region` | ROI nunca se achica mas que este rectangulo | Siempre cubre la cama aunque la persona este en una esquina |
| `max_region` | ROI nunca se expande mas que este rectangulo | No mira la puerta (y > 400) por privacidad |
| `fallback` | Que hacer si no se detecta la clase target | `"skip"` → no corre; `"full"` → corre en frame completo |

**Tabla de decision:**

| Hay persona? | Config | Comportamiento |
|---|---|---|
| No | `min_region` | Corre con `min_region` (el modelo igual se ejecuta) |
| No | sin `min_region`, `fallback = "skip"` | No corre (el cascade ya lo habria salteado) |
| No | sin `min_region`, `fallback = "full"` | Corre en frame completo |
| Si | normal | `union(persona+margin, min_region) ∩ max_region` |

---

### Perfiles y overrides por modelo

Cada modelo puede declarar un perfil y sobrescribir sus campos específicos. La
resolución combina defaults comunes, defaults del task, perfil y override local.
Tres modelos pueden compartir una política sin duplicar todo el TOML:

```toml
[models.detect-fast]              # root — sin crop, frame completo
path = "models/yolo26n.onnx"
task = "detect"
confidence = 0.5

[models.bed-detector.crop]      # siempre cubre la cama, nunca la puerta
type = "largest_class"
class = "person"
margin = 0.15
min_region = [100, 200, 500, 450]
max_region = [0, 0, 640, 400]
fallback = "full"

[models.pose-standard.crop]      # solo persona, crop justo
type = "largest_class"
class = "person"
margin = 0.15
```

---

### Interaccion con el cascade

El crop y el cascade se complementan — no se reemplazan:

```
cascade decide SI corre  →  should_run(model, &model_dets)
crop decide DONDE corre  →  resolve_crop_rect(model, &model_dets, fb)
```

**Regla para `largest_class`:** el modelo DEBE ser child en `cascade.toml` (tener `requires`). Si no, no hay parent de donde sacar detecciones.

```toml
# cascade.toml — esto es necesario para que largest_class funcione
[[rules]]
model = "pose-standard"
requires = "detect-fast"          # ← define el parent
requires_class = "person"         # ← condicion para correr
```

**Tabla de ejecucion:**

| Crop config | Sin deteccion del parent | Con deteccion |
|---|---|---|
| Sin crop | `should_run()` decide | Frame completo |
| `static` | Siempre corre con `region` | Siempre corre con `region` |
| `largest_class` (sin min ni fallback) | Skip | Crop al bbox |
| `largest_class` + `min_region` | Corre con `min_region` | Union |
| `largest_class` + `fallback = "full"` | Frame completo | Crop al bbox |

---

### Interaccion con el resto del pipeline

**No hay que tocar nada.** El engine aplica el offset automaticamente (`infer.rs:run()`):

```
deteccion en espacio del crop (10,20)
    + offset del crop (120,70)
    = deteccion en frame original (130,90)  ← esto recibe el tracker
```

- **Tracking:** si se habilita, las observaciones llegan con coordenadas originales.
  El tracker actual usa predicción lineal y matching greedy por IoU; no es todavía
  Kalman/Hungarian.
- **Zonas:** `zones.toml` usa coordenadas del frame original — compatibles sin cambios.
- **FSM:** `zone_occupied` evalua tracks en espacio original.
- **Rerun:** las cajas se renderizan sobre la imagen completa.
- **JSONL:** los bboxes se serializan en espacio original. Un `jq` no sabe que hubo crop.

---

### Como verificarlo

#### En el log de arranque

```
model detect-fast: static ROI [120,90 520,390]
model detect-fast: loaded (detect)
```

#### En el text log cada 5s

Si el crop funciona, `detect-fast` deberia mostrar menos detecciones que sin crop (solo ve la region recortada), y la latencia deberia ser menor:

```
detect-fast:  0.5 Hz | 2 calls | 12ms (10-15ms) | 4/5fr
```

Compara con correrlo sin crop — la latencia y el numero de detecciones deberian bajar.

#### En JSONL

Cada evento de deteccion incluye el crop aplicado:

```json
{"type":"detection","frame_id":42,"model":"detect-fast","infer_ms":14,"crop":[120,90,520,390],"det":[{"class":"person","confidence":0.87,"bbox":[180,150,350,380]}]}
```

El campo `crop` solo aparece cuando el modelo tiene ROI. Las coordenadas de `bbox` estan en el frame original — el bbox `[180,150,...]` cae dentro de `crop:[120,90,...]` porque la persona detectada esta dentro de la region recortada.

```bash
jq 'select(.type=="detection" and .model=="detect-fast") | {f: .frame_id, crop: .crop, n: (.det | length)}' mana-*.jsonl
```

#### En Rerun

Con `auto_views = true` en `rerun.toml`, Rerun genera las vistas automaticamente segun las entidades que recibe:

- **Camera** (`/world/camera`): frame completo con bboxes en posiciones correctas
- **Crops** (`/world/camera/crops/detect-fast`): vista separada con los pixeles exactos que recibio el modelo

Ambas vistas aparecen en pestañas separadas del blueprint. Los bboxes del modelo aparecen solo dentro de la region recortada — si el crop funciona, no hay detecciones fuera de esa zona.

---

### Troubleshooting

| Sintoma | Causa probable | Que revisar |
|---|---|---|
| El modelo hijo nunca corre | `largest_class` sin `min_region` ni `fallback` y el parent no detecta | Agregar `fallback = "full"` o `min_region` |
| Detecciones desplazadas | Bug en el offset (no deberia pasar) | `infer.rs:run()` — `d.bbox[i] += offset` |
| La latencia no baja | El crop es casi del tamano del frame | Ajustar `margin` mas chico o usar `static` |
| `max_region` no restringe | `max_region` mas grande que el frame | Verificar coordenadas en `models.toml` |
| Bboxes fuera del crop | El modelo detecta objetos en bordes del crop | Normal — el offset los mapea a frame original. Si salen del frame, revisar `imgsz` del modelo |
| El campo `crop` no aparece en JSONL | El modelo no tiene `[models.<name>.crop]` configurado | Revisar `models.toml` y confirmar que `detection_events = true` en `metrics.toml` |

---

## 5. Escenarios de configuracion

### Escenario A: Minimo — un solo modelo, sin cascade

```toml
# config/models/detect.toml (incluido por models.toml)
task = "detect"
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

Detecta presencia y activa hijos solo cuando hay personas; depth corre como
root independiente.

```toml
# Archivos incluidos por config/models.toml. Cada archivo declara un task.
# config/models/detect.toml
task = "detect"
[models.detect-fast]
path = "models/yolo26n.onnx"
task = "detect"
confidence = 0.5

# config/models/pose.toml
[models.pose-standard]
path = "tools/model-tools/artifacts/yolo26-fp16/yolo26s-pose-fp16-320.onnx"
task = "pose"
confidence = 0.3
half = true

[models.face-yolo]
path = "models/yolov12l-face.onnx"
task = "detect"
confidence = 0.10

# config/models/segment.toml
[models.seg-standard]
path = "models/yolo26x-seg-fp16-640.onnx"
task = "segment"
half = true

# config/models/depth.toml
[models.depth-standard]
path = "tools/model-tools/artifacts/yolo26-fp16/yolo26x-depth-fp16-320.onnx"
task = "depth"
half = true
[models.depth-standard.crop]
type = "static"
region = [560, 140, 1240, 820]
```

```toml
# cascade.toml
[[rules]]
model = "detect-fast"

[[rules]]
model = "depth-standard"          # root independiente, ROI fijo

[[rules]]
model = "pose-standard"
requires = "detect-fast"
requires_class = "person"
same_frame = true

[[rules]]
model = "face-yolo"
requires = "detect-fast"
requires_class = "person"
requires_exact_count = 1
same_frame = true

[[rules]]
model = "seg-standard"
requires = "detect-fast"
requires_class = "person"
same_frame = true
```

**Que esperar en Rerun:**

```
/world/camera/detections/detect-fast   ← boxes de persona
/world/camera/crops/depth-standard/depth/disparity ← mapa depth del ROI
/world/camera/detections/face-yolo     ← solo con exactamente 1 persona
```

**En logs cada 5s:**

```
infer:  2.0 Hz — 10 calls in 5s | 52ms avg | 4 dets
```

`face-yolo` y `seg-standard` solo consumen inferencia cuando `detect-fast`
detecta `person` en el mismo frame (`same_frame = true`). `depth-standard`
siempre corre sobre su ROI.

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
[health]
report_interval_s = 30          # reportar cada 30s en vez de 5s
data_stale_ms = 30000           # tolerar 30s sin frame

[pipeline]
infer = true
track = false                   # modo actual: solo consolidación stateless
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

[models.pose-standard]
enabled = false                # no necesitas keypoints en edge (ADR-020)
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

## 6. Depuracion y metricas

> **Guia completa:** [docs/observability.md](observability.md) — filosofia de los tres canales, arbol de entidades, escenarios de configuracion, y consultas forenses con jq.

### Text log cada 5s

```
ingest: 1.0 Hz — 5 keyframes processed (5 seen) in 5s | decode 15ms avg | cycles 80 | pframes:120, timeouts:80
infer:  2.0 Hz — 10 calls in 5s | 38ms avg | 2-145ms | 12 dets | skips:2, empty:1
  detect-fast:  0.4 Hz | 2 calls | 15ms avg | 10-22ms | 6/5fr
```

| Metrica | Ok | Alerta |
|---------|----|--------|
| `ingest Hz` | > 0.3 | Stream lento |
| `decode avg` | < 30ms | > 100ms → CPU |
| `infer avg` | < 50ms | > 100ms → modelo pesado |
| `X-Xms` rango | Estrecho | Mucha dispersion → inestabilidad |
| `N/Mfr` dets | > 0 | 0 persistente → modelo ciego |
| `skips` | 0 | Cascade no funciona |
| `empty` | < 30% | Threshold muy alto |

### Rerun — 8 paneles

1. **Camera** — imagen + cajas con clase y confianza
2. **Counts** — per-class detecciones por frame (flickereo = threshold mal)
3. **Confidence** — min/max por clase (max-min > 0.4 = modelo duda)
4. **Area** — min/max por clase (crece = objeto se acerca)
5. **Latency** — inferencia + decode per frame (picos = thermal throttling)
6. **Stream** — `source_hz`, `processed_hz` y `gap_ms` en unidades crudas
7. **Pipeline health** — `drop_ratio`, `throughput_ratio` y `freshness` en escala `0..1`
8. **Room state** — timeline `empty/single/multiple`, candidato de segunda
   persona y validez de la señal

La frecuencia de cada modelo vive bajo `/pipeline/infer/<model>/hz`. En el
primer experimento debe verse `detect-fast` en cada ciclo procesado y
`face-yolo` solo cuando hay exactamente una persona.

`gap_ms` no se compara directamente con `Hz`. Para una explicacion de las
formulas y de la diferencia entre valores crudos y normalizados, consultar la
seccion [Frecuencia, Gap Y Salud Normalizada](operations.md#frecuencia-gap-y-salud-normalizada)
de la guia operativa. El panel de salud normalizada es el criterio recomendado
para el dashboard; las derivaciones aun no se emiten como paths independientes
por `src/viz.rs`.

### JSONL — consultas rapidas

```bash
# Gaps de stream > 5s
jq 'select(.type=="frame" and .gap_ms > 5000)' mana-*.jsonl

# Confianza baja por clase
jq 'select(.type=="detection" and .per_class.person.conf_min < 0.5)' mana-*.jsonl

# Timeline detecciones
jq -c 'select(.type=="detection") | {f: .frame_id, m: .model, c: [.det[]?.class]}' mana-*.jsonl

# Timeline de cardinalidad de habitacion
 jq -c 'select(.type=="presence") | {f: .frame_id, room: .state, poi: .poi_state, raw: .raw_count, ton_ms: .single_timer_ms, empty_ms: .empty_timer_ms}' mana-*.jsonl
```

---

## 7. Checklist de puesta en marcha

- [ ] `models.toml`: solo los modelos que necesitas (comenta el resto)
- [ ] `cascade.toml`: cada child tiene `requires` y `requires_class` correctos
- [ ] `mana.toml` > `[inference]` > `cascade_file` apunta al archivo correcto
- [ ] Los modelos ONNX existen en `models/` y son compatibles con tu build de `ultralytics-inference`
- [ ] `[pipeline]` > `fsm = false` para empezar simple; activar FSM despues
- [ ] `enabled = false` en las ramas que no necesitas (pose, face, seg, depth)
- [ ] Rerun viewer corriendo en `127.0.0.1:9876` si `[viz] enabled = true`
- [ ] `[health] report_interval_s = 5` para desarrollo; subir a 30-60 en produccion
- [ ] Primer run: mira el log por 30s. Confirma que `ingest: X.X Hz` coincide con el GOP de la camara
- [ ] Confirma que los modelos esperados aparecen en Rerun bajo `/infer/`
- [ ] Si `skips:N` es alto y no deberia, revisa `requires_class` en `cascade.toml`
- [ ] Si `empty:N` es alto, baja `confidence` en `models.toml` para ese modelo
