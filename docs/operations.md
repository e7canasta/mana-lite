# Mana Lite Operations Guide

Guia para administradores y operadores de una instancia de Mana Lite.
Describe el modo actual de calibracion: inferencia, consolidacion de
detecciones y observabilidad, sin identidad temporal.

Para el blueprint completo de profundidad, roles, contratos ROI-locales y
procedimientos de promocion, ver [specs/depth-standard.md](specs/depth-standard.md).

## 1. Modos De Ejecucion

### Modo Actual: Consolidacion Stateless

```toml
[pipeline]
infer = true
track = false
zones = false
fsm = false
snapshot = false
```

Este modo produce:

- `detection`: salida aceptada de cada modelo.
- `consolidated_detection`: fusion espacial del frame actual.
- Rerun en `/world/camera/observations`.

No produce `track_id`, `entity` ni memoria entre frames.

### Modo Futuro: Entidades Trackeadas

```toml
[pipeline]
track = true
```

Este modo conserva las observaciones consolidadas y agrega:

- `track_id` y eventos `entity` en JSONL.
- Rerun en `/world/camera/entities`.
- Habilitacion de modelos hijos que dependen de tracks confirmados.

No activar este modo para calibrar la deteccion. Primero validar el modo
stateless con video real.

## 2. Arranque

Desde la raiz del repositorio:

```bash
cargo run -- --config config/mana.toml
```

La fuente RTSP, usuario y transporte viven en `config/mana.toml`. El punto de
entrada de modelos es `config/models.toml`; sus archivos incluidos viven en
`config/models/`. Las rutas de artefactos conservan el contrato actual y se
interpretan desde la raíz de ejecución.

## 3. Configuracion Administrativa

| Archivo | Administrar | No cambiar sin validar |
|---|---|---|
| `config/mana.toml` | RTSP, pipeline, tracking, consolidacion, salida, Rerun | Credenciales y rutas de produccion |
| `config/models.toml` y `config/models/` | ONNX, confianza, NMS, filtros, crops, perfiles | `allow_classes`, areas y `iou` |
| `config/blueprints/<name>/blueprint.toml` | Perfil activo, modelos, overlay y gates | Cambiar en 24/7 solo con validacion |
| `config/blueprints/<name>/models.toml` | Tuning local sobre el catálogo padre | Mantener `extends` apuntando al catálogo correcto |
| `[presence.poi]` en `config/mana.toml` | Histeresis de señal del POI | `on_ticks` y `off_ticks` |
| `[presence.occupancy]` en `config/mana.toml` | Timers TON/TOF de cardinalidad | `single_confirm_ms`, `empty_confirm_ms`, `multiple_confirm_ms`, `multiple_exit_ms` |
| `config/cascade.toml` | Dependencias entre modelos | `requires` y clase padre |
| `config/metrics.toml` | Resumen terminal y eventos JSONL | Desactivar eventos necesarios para diagnostico |
| `config/viz.toml` | Frames, boxes, ROI y series Rerun | `boxes` si se necesita inspeccion visual |
| `config/rerun.toml` | Blueprint y layout del viewer | Solo afecta presentacion |
| `config/zones.toml` | Zonas espaciales | Solo tiene efecto con zonas habilitadas |
| `config/fsm.toml` | Estados, guards y transiciones | Solo tiene efecto con FSM habilitada |

### Reglas De Modelos

En `models.toml`, `confidence` e `iou` pertenecen al motor del modelo.
`postprocess` pertenece al contrato de salida:

```toml
[models.detect-fast]
confidence = 0.10
iou = 0.5

[models.detect-fast.postprocess]
allow_classes = ["person", "wheelchair"]
min_confidence = 0.25
min_area_ratio = 0.001
max_area_ratio = 1.0
min_component_area_ratio = 0.0
mask_threshold = 0.5
```

El orden es:

```text
modelo -> filtros por modelo -> NMS intra-modelo -> consolidacion cross-modelo
```

No usar el NMS para resolver identidad entre modelos.

`min_area_ratio` filtra el `bbox` completo respecto al frame. En modelos de
segmentación, `min_component_area_ratio` filtra cada componente conectado de
la máscara respecto al crop de la detección. El filtro se aplica antes de
crear el `CompactMask` y los polígonos, manteniendo ambas representaciones
alineadas.

`mask_threshold` y `polygon_simplify` también son políticas de segmentación y
se configuran por modelo en `models.toml`.

### Politica De Consolidacion Face

La asociacion de face se calibra en `config/mana.toml`:

```toml
[detection]
face_component_coverage = 0.70
face_max_center_y_ratio = 0.65
```

`face_component_coverage` exige que la mayor parte del bbox face quede dentro
del bbox de `person`. `face_max_center_y_ratio` limita el centro de face a la
parte superior relativa de la persona. Ambos valores deben ajustarse con
Rerun y video RTSP real, no con imagenes sinteticas.

### Seguridad Operativa

- No guardar passwords reales en archivos versionados.
- Verificar que los paths de modelos existan antes de arrancar.
- Validar cambios de `postprocess` con un segmento de video conocido.
- Mantener `track = false` durante calibracion de detecciones.
- Activar `zones` y `fsm` solo despues de validar sus entradas.

## 4. Lectura Del Terminal

### Ingesta

```text
ingest: 1.0 Hz — 5 keyframes processed (7 seen) in 5s | decode 14ms avg | cycles 81 | pframes:26, kf_dropped:2, timeouts:81
```

- `1.0 Hz`: keyframes procesados por segundo.
- `5 keyframes processed (7 seen)`: se procesa el mas nuevo; los anteriores
  vistos durante el mismo drain no se vuelven a inferir.
- `decode 14ms avg`: tiempo medio de decodificacion.
- `pframes`: frames no usados porque `keyframes_only = true`.
- `kf_dropped`: keyframes reemplazados por otro mas reciente mientras la
  inferencia estaba ocupada.
- `timeouts`: polling sin un frame nuevo; no implica por si solo fallo RTSP.

### Flags De Alarma De Ingesta

| Flag | Significado | Que hacer |
|------|-------------|-----------|
| `dup:N` | Mismos bytes H264 repetidos | Camara congelada enviando el mismo frame. Verificar fuente. |
| `reconnect:N` | Reconexion RTSP | Se cayo la camara o red. `reconnect>2` en 5s es grave. |
| `ssrc:N` | Cambio de SSRC | El stream se reinicio (camara reboot, cambio de encoder). |
| `rtp:N` | Errores de paquete RTP | Red con perdida de paquetes. Si >10 reconsiderar TCP. |

### Inferencia

```text
infer: 1.0 Hz — 5 calls in 5s | 36 (35-36ms) | 4 dets | skips:5, empty:1
```

- `5 calls`: invocaciones de modelos en la ventana.
- `36 (35-36ms)`: promedio y rango de latencia.
- `4 dets`: detecciones aceptadas acumuladas.
- `skips`: modelos omitidos por la cascada.
- `empty`: inferencias sin detecciones aceptadas.

### Linea Por Modelo

```text
detect-fast: 1.0 Hz | 5 calls | 36 (35-36ms) | 4/5fr | empty,roi:[420,0 1500,1080]
```

`4/5fr` significa que hubo detecciones aceptadas en cuatro de cinco frames.
`empty` significa que al menos una llamada no produjo salida despues de los
filtros. `roi` muestra el crop aplicado.

### Frecuencia, Gap Y Salud Normalizada

`Hz` y `gap_ms` son metricas crudas con unidades distintas. No deben
compartir el mismo eje de un grafico. `gap_ms` conserva el valor absoluto para
diagnostico; para comparar salud del pipeline se usan metricas relativas.

```text
expected_period_ms = 1000 / source_hz
gap_ratio          = gap_ms / expected_period_ms
drop_ratio         = dropped / seen
throughput_ratio   = min(1.0, processed_hz / source_hz)
freshness          = min(1.0, expected_period_ms / gap_ms)
```

Interpretacion:

| Metrica | Rango esperado | Significado |
|---|---|---|
| `source_hz` | `0..N` Hz | Keyframes observados por el demuxer |
| `processed_hz` | `0..N` Hz | Keyframes que llegaron a procesamiento |
| `gap_ms` | `0..N` ms | Tiempo absoluto entre keyframes procesados |
| `gap_ratio` | `0..N` | `1.0` es un periodo esperado; `>1.0` indica atraso |
| `drop_ratio` | `0..1` | Fraccion de keyframes reemplazados por uno mas nuevo |
| `throughput_ratio` | `0..1` normalmente | Fraccion del ritmo de fuente que se procesa |
| `freshness` | `0..1` | `1.0` es fresco; valores bajos indican atraso |

Si un denominador es cero, no emitir la muestra normalizada. La division se
debe hacer despues de verificar `source_hz > 0`, `seen > 0` y `gap_ms > 0`.

Ejemplo con una fuente de `1 Hz`: un `gap_ms` de `999` representa un
`gap_ratio` de `0.999`, que es normal. Si el gap es `2400 ms`, el ratio es
`2.4` y el pipeline se atraso mas de dos periodos.

Para un grafico comun usar `drop_ratio`, `throughput_ratio` y `freshness`.
Mantener `source_hz`, `processed_hz` y `gap_ms` en paneles con sus unidades
originales. Estas metricas normalizadas son derivaciones de observabilidad;
los paths crudos actuales estan listados en la tabla de Rerun.

### Postprocesado

```text
model detect-fast: postprocess rejected=4 nms_suppressed=0
```

- `rejected`: candidatos eliminados por clase, confianza, area o geometria.
- `nms_suppressed`: candidatos eliminados por NMS o por el limite
  `max_detections` del mismo modelo.
- Ninguno de los dos significa que se haya creado una entidad nueva.

Los parametros de postprocesado son independientes por modelo en
`config/models.toml`:

```toml
[models.face-yolo.postprocess]
allow_classes = ["face"]
min_confidence = 0.20
min_area_ratio = 0.0001
max_area_ratio = 0.25
nms_iou = 0.05
max_detections = 1
```

`models.<name>.iou` controla el NMS del backend Ultralytics. El campo
`postprocess.nms_iou` controla el NMS explicito de mana-lite despues de aplicar
clase, confianza y area. Este NMS ordena por confianza descendente y conserva
la deteccion de mayor confianza cuando hay solapamiento de la misma clase.
Para `face-yolo`, el valor bajo elimina duplicados muy cercanos de una misma
persona. `max_detections` es una segunda regla opcional posterior al NMS:
conserva solo las detecciones de mayor confianza hasta ese limite. Para el
experimento actual, `face-yolo` conserva una sola face por frame; se puede
eliminar o aumentar este campo cuando pose o segmentacion pasen a resolver los
casos ambiguos.

### Por Que Pose Aparece Como Skip

Con `track = false`, los modelos hijos que requieren un track confirmado no
tienen target valido y se omiten. Es esperado ver:

```text
pose-standard: 0.0 Hz | 0 calls | ... | skip
```

Para probar pose primero se debe habilitar tracking o configurar una ejecucion
root independiente que no dependa de un track.

### Cascada Actual

La topologia de `config/cascade.toml` es:

```text
detect-fast (root)
  ├── pose-standard   same_frame, requiere person en region bed
  ├── face-yolo       same_frame, requiere exactamente 1 person
  └── seg-standard    same_frame, requiere person
depth-standard (root independiente, ROI fijo [560,140 1240,820])
```

`face-yolo` usa `models/yolov12l-face.onnx`. No es un modelo root: en la
cascada generica puede usar `same_frame = true`; el blueprint activo
`detect-room-face` usa el track confirmado del padre para construir el crop
facial dinamico. En ambos casos solo se ejecuta con exactamente una persona.

La deteccion `face` tiene un contrato distinto al de una deteccion primaria:

- No crea una persona ni una identidad nueva.
- Se consolida como componente de `person` solo si su bbox queda contenido en
  la bbox de la persona y su centro esta en la mitad superior del cuerpo.
- Si no encuentra una persona compatible, se descarta de la consolidacion
  primaria; queda disponible en el evento `detection` crudo para diagnostico.
- Con cero o dos personas, `face-yolo` aparece como `skip` y no consume
  inferencia.
- El crop de face es un cuadrado dinamico centrado en la mitad superior de la
  persona y puede sobresalir del ROI fijo de `detect-fast` (ADR-023). La ROI
  fija `face_dwell` de 400x300, la zona semantica `zones.bed` y el ROI fijo de
  `depth-standard` son regiones independientes.

`depth-standard` no entra en consolidacion, tracking, zonas ni FSM; su
validacion es `valid_pixels` y estadisticas, no detecciones. Blueprint
completo en [specs/depth-standard.md](specs/depth-standard.md).

## 5. JSONL

El JSONL es la salida forense. En `config/mana.toml`:

```toml
[output]
format = "jsonl"
save_dir = "./logs"
rotate = "hourly"
jsonl_level = "debug"
```

Eventos relevantes:

| Evento | Identidad | Uso |
|---|---|---|
| `detection` | No | Diagnostico por modelo |
| `consolidated_detection` | No | Resultado stateless del frame |
| `depth` | No | Estadisticas del mapa depth (valid_pixels, min/max) |
| `entity` | Si, `track_id` | Tracking temporal |
| `meta` con `track_*` | Si el tracking esta activo | Lifecycle del tracker |

Consultas utiles:

```bash
jq 'select(.type == "detection")' logs/*.jsonl
jq 'select(.type == "consolidated_detection")' logs/*.jsonl
jq 'select(.type == "consolidated_detection" and .class == "person")' logs/*.jsonl
jq 'select(.type == "frame" and .gap_ms > 5000)' logs/*.jsonl
jq 'select(.type == "detection" and .post_rejected > 0)' logs/*.jsonl
```

Para revisar un frame especifico:

```bash
jq 'select(.frame_id == 791)' logs/*.jsonl
```

## 6. Rerun Y Blueprint

El proceso intenta conectarse a `127.0.0.1:9876` cuando `[viz].enabled = true`.
Abrir el viewer Rerun compatible y conectarlo a esa direccion antes o despues
de arrancar Mana Lite.

El blueprint de `config/rerun.toml` controla el layout del viewer, no la
inferencia. Los paths de datos los escribe `src/viz.rs`:

| Path | Contenido | Modo |
|---|---|---|
| `/world/camera/bgr` | Frame RGB | Siempre que `frames = true` |
| `/world/camera/observations` | Boxes consolidadas | Siempre que `boxes = true` |
| `/world/camera/entities` | Boxes con identidad | Solo `track = true` |
| `/world/camera/rois/<model>` | Crop ROI | Si `roi_rects = true` |
| `/world/camera/crops/<model>/bgr` | Imagen del crop | Si `crop_frames = true` |
| `/world/camera/crops/<model>/detections` | Detecciones en espacio local del crop | Si `boxes = true` |
| `/world/camera/crops/<model>/mask` | Overlay de mascara del crop | Si `masks = true` |
| `/world/camera/detections/<model>` | Detecciones crudas en espacio frame | Si `boxes = true` |
| `/world/camera/detections/<model>/pose` | Keypoints + skeleton de pose | Si `boxes = true` |
| `/world/camera/masks/<model>` | Overlay de mascaras | Si `masks = true` |
| `/world/camera/crops/depth-standard/depth/{disparity,annotated}` | Mapa depth local | Si `crop_frames`/`frames` habilitan el flujo |
| `/world/camera/depth/depth-standard/stats/valid_pixels` | Pixeles validos del mapa | Con depth activo |
| `/ingest/keyframes/source_hz` | Hz estimado de keyframes vistos | Si `keyframe_rate = true` |
| `/ingest/keyframes/processed_hz` | Hz de keyframes procesados | Si `keyframe_rate = true` |
| `/ingest/keyframes/dropped` | Keyframes reemplazados por uno mas nuevo | Si `keyframe_drops = true` |
| `/pipeline/infer/<model>/latency_us` | Tiempo de inferencia reportado por backend | Si `infer_latency = true` |
| `/pipeline/infer/<model>/pipeline_us` | Tiempo wall-clock del modelo completo | Si `infer_latency = true` |
| `/pipeline/infer/<model>/hz` | Frecuencia de llamadas del modelo | Si `infer_rate = true` |
| `/pipeline/decode/latency_us` | Tiempo de decode | Si `decode_latency = true` |
| `/pipeline/state/room/cardinality` | Timeline `empty/single/multiple` | Siempre con Rerun conectado |
| `/pipeline/state/room/second_person` | Timeline `none/candidate/confirmed` | Siempre con Rerun conectado |
| `/pipeline/state/room/signal` | Timeline `valid/invalid` | Siempre con Rerun conectado |

Metricas derivadas recomendadas para el dashboard, aun no emitidas como paths
independientes por `src/viz.rs`:

| Metrica | Formula | Uso |
|---|---|---|
| `drop_ratio` | `dropped / seen` | Medir cuanto trabajo se descarta |
| `throughput_ratio` | `min(1, processed_hz / source_hz)` | Medir si el pipeline sigue la fuente |
| `freshness` | `min(1, expected_period_ms / gap_ms)` | Escala comun de frescura |

Configuracion minima para calibrar:

```toml
[viz]
enabled = true
rerun_addr = "127.0.0.1:9876"

[viz.send]
frames = true
boxes = true
crop_frames = false
roi_rects = true
keyframe_gap = true
keyframe_rate = true
keyframe_drops = true
infer_latency = true
decode_latency = true
```

En el modo actual se debe inspeccionar `/world/camera/observations`. Las
boxes son frame-locales: pueden desaparecer cuando el detector no produce una
observacion y eso no representa una perdida de identidad, porque no hay
tracking activo.

## 7. Troubleshooting

| Sintoma | Verificar | Accion |
|---|---|---|
| `infer: 0 calls` | Modelo, path y `pipeline.infer` | Revisar startup y `models.toml` |
| `empty` frecuente | Confianza, allowlist, area, crop | Revisar `postprocess` y Rerun |
| Muchos `rejected` | `min_confidence`, clase y area | Comparar candidatos crudos con filtros |
| Muchos `nms_suppressed` | `iou` del modelo | Ajustar solo despues de validar duplicados intra-modelo |
| Muchos `kf_dropped` | Latencia de inferencia y `throughput_ratio` | Aceptable durante carga; reducir costo si es sostenido |
| `pose ... skip` | `track = false` o sin track confirmado | Esperado en modo stateless |
| No hay boxes en Rerun | `[viz].enabled`, `viz.send.boxes`, conexion | Revisar direccion y blueprint |
| No hay JSONL | `save_dir`, `jsonl_level`, permisos | Revisar startup y archivo rotado |
| Muchos timeouts | Salud del RTSP y GOP | Distinguir timeout de error RTP |

## 8. Checklist De Operacion

- [ ] `cargo test` pasa antes de desplegar.
- [ ] Los modelos configurados existen y cargan.
- [ ] Usar `detect-room-raw` y `track = false` para calibrar cardinalidad raw.
- [ ] Usar `detect-room-face` con `track = true`, `zones = true` y `fsm = true` para validar la FSM facial.
- [ ] `detection`, `consolidated_detection` y `presence` aparecen en JSONL.
- [ ] Rerun muestra `/world/camera/observations`.
- [ ] Rerun muestra la timeline `/pipeline/state/room`.
- [ ] Rerun muestra la timeline `/pipeline/state/face` y el crop dinamico `/world/camera/crops/face-yolo/bgr`.
- [ ] La tasa de keyframes vistos y procesados es interpretable.
- [ ] Los drops de keyframes ocurren cuando la inferencia se alarga, sin backlog creciente.
- [ ] `gap_ms` y `Hz` se visualizan en paneles separados; la salud usa ratios `0..1`.
- [ ] `empty`, `rejected` y `nms_suppressed` se interpretan por separado.
- [ ] `face-yolo` solo corre cuando `detect-fast` encuentra exactamente una persona.
- [ ] La cara se consolida como componente de `person`, no como entidad primaria equivalente.
- [ ] Las credenciales no estan en git.
- [ ] Tracking se habilita solo en una prueba posterior y controlada.
