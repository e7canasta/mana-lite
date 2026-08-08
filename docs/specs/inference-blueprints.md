# Spec-004: Inference Blueprints

**Status:** Accepted
**Scope:** seleccion de modelos y reglas de activacion

## 1. Proposito

Un blueprint es una configuracion nombrada de despliegue. Selecciona los
modelos del catalogo que se cargaran, declara el modelo primario y define las
reglas que activan los modelos secundarios.

El blueprint no cambia el catalogo de modelos. Un mismo catalogo puede tener
varios blueprints.

```text
catalogo disponible
        |
        v
blueprint seleccionado por mana.toml
        |
        v
modelos activos + cascade + gates
```

## 2. Planos de configuracion

### Catalogo

`config/models.toml` describe artefactos y defaults tecnicos del runtime:

- path del ONNX.
- task.
- parametros de inferencia.
- postprocess.
- crop.

El campo `enabled` del formato actual es un mecanismo heredado. Cuando existe
un blueprint, la lista `blueprint.models` determina los modelos activos para
esa ejecucion.

### Blueprint

`config/blueprints/<name>/blueprint.toml` describe una instancia operativa:

```toml
[blueprint]
name = "detect-face-pose-seg"
description = "..."
primary_model = "detect-fast"
models = ["detect-fast", "face-yolo", "pose-standard", "seg-standard"]
requires_tracking = true

[[rules]]
model = "detect-fast"

[[rules]]
model = "face-yolo"
requires = "detect-fast"
requires_class = "person"
requires_exact_count = 1
requires_min_confidence = 0.50
```

`models` es una lista de activacion, no una copia del catalogo. Un modelo
deshabilitado en el catalogo global puede ser activado explicitamente por un
blueprint seleccionado.

### Configuracion operativa

`config/mana.toml` selecciona el blueprint:

```toml
[inference]
model_catalog = "config/models.toml"
blueprint_file = "config/blueprints/detect-face/blueprint.toml"
```

La fuente RTSP, health, salida, tracking y visualizacion siguen siendo
configuracion de instancia y permanecen en `mana.toml`.

## 3. Flujo de resolucion

Durante el bootstrap:

1. Se carga el catalogo completo.
2. Se carga el blueprint seleccionado.
3. Se valida que los modelos del blueprint existan.
4. Se valida que el modelo primario este seleccionado y sea root.
5. Se crea un catalogo runtime con solo los modelos seleccionados habilitados.
6. Se cargan las sesiones de esos modelos.
7. Se ordenan las reglas root antes que sus hijos.
8. En cada frame, la cascada decide que child puede ejecutar.

La estructura fija del runtime no es configurable como grafo arbitrario:

```text
INGEST -> DECODE -> INFERENCE STAGES -> CONSOLIDATION
                                      -> TRACK -> ZONES -> FSM -> PUBLISH
```

## 4. Gates de activacion

### Root

Una regla sin `requires` es root. Un root corre cuando:

- esta en `blueprint.models`.
- esta habilitado en `mana.toml`.
- el pipeline de inferencia esta activo.

Ser root no significa ignorar el blueprint ni la configuracion operativa.

### `same_frame = true`

El child usa las detecciones del parent producidas en el frame actual. No
requiere tracking y es util para calibracion o despliegues de baja latencia.

El flujo es:

```text
detect-fast -> detecciones aceptadas -> gates -> face/pose/seg
```

El riesgo es que un falso positivo de un solo frame active el child. Por eso
este modo debe usar filtros de confianza, area y conteo.

### Gate basado en track

Cuando `same_frame` es falso, el child necesita un track confirmado y visible.
El filtro de presencia puede mantener la ultima observacion durante un dropout
corto antes de entregarla al tracker:

```text
detect-fast -> deteccion -> tracker -> track confirmado -> child
```

El track debe tener:

- clase requerida.
- `is_confirmed = true`.
- `misses = 0` mientras la presencia esta fresca o siendo sostenida por el
  filtro temporal.
- confianza y area suficientes.
- conteo exacto si se declara `requires_exact_count`.

Este es el modo recomendado para 24/7 cuando los children son caros o una
activacion por parpadeo es clinicamente indeseable.

## 5. Regla de una persona

Para ejecutar face, pose y segmentacion solo cuando hay una persona en la
escena:

```toml
[[rules]]
model = "pose-standard"
requires = "detect-fast"
requires_class = "person"
requires_exact_count = 1
requires_min_confidence = 0.50
requires_min_area_ratio = 0.01
```

En un blueprint estable esta regla usa tracking. `min_hits` confirma la
persona y el filtro temporal sostiene vacios cortos antes de declarar ausencia.

El conteo se realiza sobre la escena efectiva del detector, incluyendo su ROI
configurado. Si el detector procesa solo una region de la imagen, "una persona
en la sala" significa una persona en esa region.

## 6. Filtro temporal de presencia / senal

El filtro se ejecuta despues de la consolidacion espacial de las observaciones
primarias y antes de actualizar el tracker. Solo cuenta ticks con inferencia
valida:

```toml
[presence]
enabled = true
class = "person"

[presence.poi]
on_ticks = 1
off_ticks = 8

[presence.occupancy]
single_confirm_ms = 3000
empty_confirm_ms = 8000
multiple_confirm_ms = 5000
multiple_exit_ms = 5000
require_confirmed_tracks = false
```

`poi` y `occupancy` son politicas distintas. El mecanismo recibe la evidencia
del frame y aplica cada politica en su propia capa:

- `poi` es permisiva: adquiere rapido la persona de interes y sostiene su bbox
  durante dropouts cortos antes del tracker. `on_ticks` es el TON de entrada:
  la primera deteccion inicia el timer y la presencia solo se confirma tras
  evaluaciones validas sostenidas.
- `occupancy` es conservadora para confirmar una segunda persona: requiere
  `multiple_confirm_ms` de conteo `2+`. Puede exigir tracks confirmados con
  `require_confirmed_tracks = true`; el perfil raw de calibracion lo deja en
  `false`.
- `multiple_exit_ms` evita volver a `single` por una perdida aislada de la
  segunda persona.

La maquina de cardinalidad publica estados independientes del FSM clinico y
usa tiempo monotono, no cantidad de keyframes:

```text
EMPTY   + persona confirmada           -> SINGLE
SINGLE  + segundo candidato breve       -> SINGLE
SINGLE  + segundo candidato persistente -> MULTIPLE
MULTIPLE + perdida breve de la segunda -> MULTIPLE
MULTIPLE + salida confirmada           -> SINGLE o EMPTY
sin frame valido                       -> no cambia el estado
```

La observacion sostenida no crea una identidad nueva. Solo evita que un hueco
breve de la senal haga perder la presencia y permite que el tracker continue
con su continuidad espacial. El `track_id`, la prediccion y el matching siguen
siendo responsabilidad del tracker clasico.

La maquina se visualiza en Rerun con `StateChange` y `StateTimelineView` bajo:

```text
/pipeline/state/room/cardinality
/pipeline/state/room/second_person
/pipeline/state/room/signal
```

## 7. Anti-parpadeo

El sistema aplica estabilidad en capas:

1. Postprocess del modelo elimina candidatos de baja confianza o area.
2. `TrackerConfig.min_hits` evita confirmar un candidato aislado.
3. El cascade exige track confirmado y visible para children estables.
4. `requires_exact_count = 1` bloquea la rama si hay cero o mas de una persona.
5. El filtro de presencia sostiene vacios cortos y corta despues de
   `off_ticks`.
6. `max_age` conserva identidad para tracking y evita borrar de inmediato el
   track.

No se debe usar `max_age` como permiso para ejecutar un modelo hijo sobre una
posicion vieja. La frescura de la evidencia y la elegibilidad del child son
conceptos distintos.

## 8. Blueprints incluidos

### `detect-face`

```text
detect-fast -> face-yolo
```

Usa tracking y filtro de presencia para una habitacion con una persona.

### `detect-face-pose-seg`

```text
                 -> face-yolo
detect-fast      -> pose-standard
                 -> seg-standard
```

Exige tracking, una persona confirmada y visible, y activa las tres ramas
secundarias solo cuando el conteo exacto es uno.

## 9. Futuro: presets separados

La separacion completa prevista es:

```text
models.toml       catalogo de artefactos
presets.toml      defaults de inferencia y postprocess
blueprint.toml    stages, referencias y gates
mana.toml         operacion 24/7
```

La implementacion actual introduce blueprints sin romper el esquema existente
de `models.toml`. La extraccion de presets sera una migracion posterior y no
debe cambiar la semantica de los gates.
