# ROI — Region of Interest crops per model

Guia conceptual y practica del sistema de crop pre-inferencia. Para funcionales que configuran modelos y para integradores que entienden el mecanismo interno.

---

## Indice

1. [Por que ROI](#1-por-que-roi)
2. [Como funciona — el pipeline mental](#2-como-funciona--el-pipeline-mental)
3. [Las dos fuentes de ROI](#3-las-dos-fuentes-de-roi)
4. [Las tres politicas](#4-las-tres-politicas)
5. [Independencia por modelo](#5-independencia-por-modelo)
6. [Coordenadas — garantia del frame original](#6-coordenadas--garantia-del-frame-original)
7. [Interaccion con el cascade](#7-interaccion-con-el-cascade)
8. [Interaccion con tracking, zonas y FSM](#8-interaccion-con-tracking-zonas-y-fsm)
9. [Recetas practicas](#9-recetas-practicas)
10. [Como verificarlo](#10-como-verificarlo)

---

## 1. Por que ROI

Tres razones para no pasarle el frame completo al modelo:

| Razon | Ejemplo | Sin ROI | Con ROI |
|---|---|---|---|
| **Velocidad** | Pose sobre persona | 640x480 → toda la escena | 200x400 → solo la persona |
| **Precision** | Face sobre cabeza | Ruido de fondo confunde al modelo | Solo la region de la cara |
| **Privacidad** | No mirar la puerta | Puerta en el frame → detecciones | `max_region` la excluye |

Un crop 2x mas chico es ~4x menos pixeles → el modelo tarda menos y se distrae menos con fondo.

---

## 2. Como funciona — el pipeline mental

```
Frame 640x480
    │
    ▼
┌─────────────────────────────────┐
│  Modelo padre (ej: detect-fast) │  ← corre en frame completo
│  detecta: person @ [150,100,350,400]
└─────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────┐
│  resolve_crop_rect()            │
│  ┌──────────────────────────┐   │
│  │ class="person"           │   │
│  │ margin=0.15              │   │
│  │ → [120,70,380,430]       │   │
│  │ ∪ min_region            │   │
│  │ ∩ max_region            │   │
│  └──────────────────────────┘   │
└─────────────────────────────────┘
    │
    ▼  crop_rect = (120,70,380,430)
┌─────────────────────────────────┐
│  Modelo hijo (ej: pose)         │
│  RGB[120:380, 70:430] → ONNX    │
│  detecciones + offset → frame   │
└─────────────────────────────────┘
    │
    ▼  coordenadas en espacio original
Tracking, zonas, FSM — sin cambios
```

**Regla de oro:** el modelo nunca sabe que hubo crop. Recibe una imagen mas chica, produce detecciones en ese espacio, y el engine suma `offset_x/offset_y` para devolverlas al espacio del frame original.

---

## 3. Las dos fuentes de ROI

### Static — rectangulo fijo en TOML

Para camaras fijas que miran un area conocida. No depende de detecciones.

```toml
[models.detect-fast.crop]
type = "static"
region = [100, 50, 500, 400]  # x1,y1,x2,y2 en pixels
```

**Cuando usarlo:** root model con camara apuntando a una cama, puerta, o mesa fija. El modelo solo mira esa zona — todo lo de afuera no existe.

**Internamente:** el ROI se pasa a `InferenceConfig::with_roi()` al cargar el modelo. El preprocessing del engine hace el crop + offset. Las detecciones salen en coordenadas originales automaticamente.

### LargestClass — dinamico, desde detecciones del parent

El ROI se calcula por frame, basado en el bbox mas grande de una clase detectada por el modelo padre.

```toml
[models.pose-standard.crop]
type = "largest_class"
class = "person"
margin = 0.15
```

**Cuando usarlo:** modelo hijo que solo debe correr donde el padre encontro algo. Ej: pose solo donde hay persona, face solo donde hay persona.

**Calculo por frame:**
1. Buscar en `model_dets[parent]` todas las detecciones de `class`
2. Elegir la de mayor area (`max_by(area)`)
3. Expandir `margin * bbox_size` en cada direccion
4. Clampear a bordes del frame

---

## 4. Las tres politicas

Todas aplican solo a `type = "largest_class"`:

```toml
[models.bed-detector.crop]
type = "largest_class"
class = "person"
margin = 0.15
min_region = [100, 200, 500, 450]   # nunca mas chico que esto
max_region = [0, 0, 640, 400]       # nunca mas grande que esto
fallback = "full"                    # "skip" (default) o "full"
```

### `min_region` — piso

El ROI nunca se achica mas que esta region. Util para modelos que deben cubrir un area fija aunque la persona se mueva.

| Escena | ROI resultante |
|---|---|
| No hay persona | Solo `min_region` (el modelo igual corre) |
| Persona en una esquina de la cama | `union(persona+margin, min_region)` = toda la cama |
| Persona ocupa toda la cama | Union, mismo area |
| Persona en otra parte de la habitacion | Union, cubre cama + persona |

### `max_region` — techo

El ROI nunca excede esta region. Util para privacidad o para limitar el gasto de computo.

| Escena | ROI resultante |
|---|---|
| Persona centrada en el frame | `persona+margin` — todo ok |
| Persona cerca de la puerta | `persona+margin ∩ max_region` — la puerta queda fuera |
| `min_region` + `max_region` juntos | ROI ∈ [`min_region` ∪ ..., `max_region`] |

### `fallback` — que hacer sin detecciones

| Valor | Sin detecciones del parent |
|---|---|
| `"skip"` (default) | El modelo no corre. El cascade ya dijo que no. |
| `"full"` | Corre en el frame completo. Util si el modelo sirve aunque no haya persona. |

`fallback = "full"` tiene sentido para modelos que no son estrictamente dependientes: "si hay persona, enfocate ahi; si no, mira todo por si acaso".

---

## 5. Independencia por modelo

Cada entry en `models.toml` tiene su propia seccion `[models.<name>.crop]`. No se heredan, no se comparten.

```toml
[models.detect-fast]            # root — sin crop, frame completo
path = "models/yolo26n.onnx"
task = "detect"
confidence = 0.5
imgsz = 640

[models.bed-detector.crop]    # siempre cubre la cama
type = "largest_class"
class = "person"
margin = 0.15
min_region = [100, 200, 500, 450]
max_region = [0, 0, 640, 400]
fallback = "full"

[models.pose-standard.crop]    # solo persona, crop justo
type = "largest_class"
class = "person"
margin = 0.15

[models.face-v11.crop]         # margen mas chico, sin min_region
type = "largest_class"
class = "person"
margin = 0.10
```

Tres modelos, tres politicas distintas. El cascade define **quien es el parent** (de donde vienen las detecciones para el ROI), y cada modelo define **como** usar esa informacion.

---

## 6. Coordenadas — garantia del frame original

Todas las detecciones, sin importar el crop, salen en coordenadas del frame original de 640x480 (o la resolucion de la camara).

```
Frame 640x480
    ├── crop a [100,100 → 300,400]
    │   ├── modelo infiere en espacio 200x300
    │   ├── deteccion en (10,20) del crop
    │   └── + offset (100,100) → (110,120) en el frame ← esto es lo que ve el tracker
    │
    └── el tracker, zonas, FSM, JSONL — todos reciben (110,120)
```

**No hay que hacer nada.** El engine lo resuelve:
- `infer.rs:run()`: `d.bbox[0] += offset_x`, `d.bbox[1] += offset_y`, etc.
- para static, el propio `ultralytics-inference` aplica `roi_offset` en postprocessing

---

## 7. Interaccion con el cascade

El cascade y el crop interactuan en dos puntos:

### Decision de correr o no

```rust
let crop_rect = resolve_crop_rect(model, &model_dets, fb);
let always_run = crop_info(model).map_or(false, |c| c.always_run());

if crop_rect.is_none() && !always_run && !cascade.should_run(model, &model_dets) {
    tick_infer_skip(model);
    continue;
}
```

| Crop config | Sin deteccion del parent | Con deteccion |
|---|---|---|
| Sin crop | `should_run()` decide | `should_run()` decide + frame completo |
| `largest_class` sin `min_region` ni `fallback=full` | Skip | Crop al bbox |
| `largest_class` + `min_region` | Corre con `min_region` | Corre con union |
| `largest_class` + `fallback=full` | Corre en frame completo | Crop al bbox |
| `static` | Siempre corre con `region` | Siempre corre con `region` |

### Parent para `largest_class`

El parent se determina por el cascade: `cascade.parent_of("pose-standard")` devuelve `"detect-fast"`. Si el modelo no tiene `requires` en `cascade.toml`, no hay parent → `largest_class` no funciona (no hay de donde sacar detecciones).

**Regla:** si usas `largest_class`, el modelo DEBE ser child en `cascade.toml` (tener `requires`).

---

## 8. Interaccion con tracking, zonas y FSM

**Tracking:** las detecciones que recibe el tracker ya estan en coordenadas originales. IOU matching, Kalman, todo funciona igual.

**Zonas:** las zonas se definen en `zones.toml` en coordenadas del frame original. El offset del crop ya fue aplicado → un `person` en la zona `bed` se detecta igual con o sin crop.

**FSM:** los guards (`zone_occupied`, `zone_vacated`) evaluan sobre tracks en coordenadas originales. Sin cambios.

**Viz/Rerun:** las cajas se renderizan en el espacio original. El crop es transparente para la visualizacion.

**JSONL:** las detecciones se serializan con coordenadas originales. Un analisis post-hoc con `jq` no necesita saber que hubo crop.

---

## 9. Recetas practicas

### Receta 1: Camara fija mirando una cama

```toml
[models.detect-fast]
path = "models/yolo26n.onnx"
task = "detect"
confidence = 0.5
imgsz = 640
[models.detect-fast.crop]
type = "static"
region = [80, 150, 560, 430]   # solo la cama, ignorar paredes
```

El modelo corre ~40% mas rapido (menos pixeles) y no se distrae con objetos en los bordes.

### Receta 2: Pose solo donde hay persona

```toml
# cascade.toml
[[rules]]
model = "detect-fast"
[[rules]]
model = "pose-standard"
requires = "detect-fast"
requires_class = "person"

# models.toml
[models.pose-standard.crop]
type = "largest_class"
class = "person"
margin = 0.15
```

Sin persona → `pose-standard` no corre (cascade lo saltea). Con persona → crop al bbox + 15% margen → mas rapido y preciso.

### Receta 3: Detector de cama con privacidad

```toml
[models.bed-detector]
path = "models/yolo26s.onnx"
task = "detect"
confidence = 0.4
imgsz = 640
[models.bed-detector.crop]
type = "largest_class"
class = "person"
margin = 0.15
min_region = [100, 200, 500, 450]   # siempre cubre la cama
max_region = [0, 0, 640, 400]       # nunca mira la puerta (y=400..480)
fallback = "full"                    # sin persona, igual corre en la cama
```

Comportamiento:
- Habitacion vacia → ROI = cama (`min_region`) → detecta si la cama esta ocupada
- Persona entra → ROI = union(persona, cama) ∩ max_region → ve persona + cama, no la puerta
- Persona sale → vuelve a `min_region`

### Receta 4: Face con margen ajustado

```toml
# cascade.toml
[[rules]]
model = "face-v11"
requires = "detect-fast"
requires_class = "person"

# models.toml
[models.face-v11.crop]
type = "largest_class"
class = "person"
margin = 0.10    # margen mas chico — la cara es mas chica que el cuerpo
```

Sin `min_region`: si no hay persona, el modelo no corre. El margen de 10% es suficiente para capturar la cara aunque la persona se mueva.

### Receta 5: Root model con crop + child models sin crop

```toml
[models.detect-fast.crop]
type = "static"
region = [0, 0, 640, 400]   # root solo mira la mitad superior

[models.pose-standard]       # sin crop — usa el frame completo
path = "models/yolo26n-pose.onnx"
task = "pose"
confidence = 0.3
```

`detect-fast` solo ve la mitad superior. `pose-standard` corre en frame completo. Las detecciones de `detect-fast` (en espacio recortado) se usan para decidir si `pose-standard` corre — el cascade evalua `should_run` con coordenadas ya mapeadas al original.

---

## 10. Como verificarlo

### En el text log

Cuando un modelo tiene ROI, lo ves al inicio:

```
model detect-fast: loaded (detect)
model detect-fast: static ROI [80,150 560,430]
model pose-standard: loaded (pose)
```

### En Rerun

Los bounding boxes del modelo con crop aparecen en la posicion correcta del frame original. Si el crop funciona, deberias ver:
- Menos cajas en los bordes (si usas static crop)
- Cajas agrupadas alrededor de la persona (si usas largest_class)
- Sin cajas en la region de la puerta (si usas max_region)

### En JSONL

Las detecciones tienen coordenadas del frame original. Para verificar que un modelo con crop produce detecciones correctas:

```bash
jq 'select(.type=="detection" and .model=="pose-standard") | {f: .frame_id, det: [.det[] | {c: .class, bbox: .bbox}]}' mana-*.jsonl
```

Los bboxes deben estar dentro de [0, frame_w] x [0, frame_h] — si estan recortados o desplazados, el offset no se aplico.

### Troubleshooting

| Sintoma | Causa probable | Que revisar |
|---|---|---|
| El modelo hijo nunca corre | `fallback = "skip"` sin `min_region` y el parent no detecta la clase | Agregar `fallback = "full"` o `min_region` |
| Detecciones desplazadas | Bug en el offset (no deberia pasar) | Revisar `infer.rs:run()` — `d.bbox[i] += offset` |
| El modelo es mas lento, no mas rapido | El crop es casi del tamano del frame → sin ganancia | Ajustar `margin` o usar `static` en vez de `largest_class` |
| `max_region` no parece funcionar | El `max_region` es mas grande que el frame → no restringe nada | Verificar coordenadas en `models.toml` |
| `min_region` cubre zonas que no deberia | Union con `class_rect` incluye areas no deseadas | Agregar `max_region` o reducir `margin` |
