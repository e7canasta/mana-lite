# Handoff: Integración de YOLO26 Depth

**Estado:** integración base implementada y prueba depth-only activa contra la
cámara RTSP configurada.

**Objetivo:** incorporar los modelos YOLO26 de profundidad como una salida de
primera clase del pipeline y visualizar el mapa en Rerun, sin convertirlo en una
detección, una entidad, una máscara ni una evidencia que el consolidator pueda
fusionar.

## Contexto De La Sesión

### Assets disponibles

- Los modelos depth originales `.pt` y los ONNX FP32 se obtienen desde
  Ultralytics.
- Ya se generaron los artefactos FP16 para `s`, `m`, `l` y `x`, en `320` y `640`:

  ```text
  tools/model-tools/artifacts/yolo26-fp16/
    yolo26{s,m,l,x}-depth-fp16-320.onnx
    yolo26{s,m,l,x}-depth-fp16-640.onnx
  ```

- Estos artefactos ya verifican que el grafo contiene inicializadores
  `FLOAT16`.
- `models/` es un symlink a `/home/care/opt/workspace/references/inference`.
- `yolo26s-depth-fp16-320.onnx` ya fue promovido al repositorio runtime y está
  activo para esta prueba.
- No hace falta regenerar los modelos para comenzar este sprint.

### Estado activo de `mana-lite`

En la última prueba quedaron activos:

- `detect-fast`: `yolo26x-fp16-640.onnx`.
- `seg-standard`: `yolo26x-seg-fp16-640.onnx`.
- `pose-standard`: `yolo26s-pose-fp16-320.onnx`.
- `face-yolo`: `yolov12l-face.onnx`.
- ROI fija de prueba: `[560, 140, 1240, 820]` (`680x680`).

La prueba real confirmó que los cuatro modelos cargan. La detección `xlarge`
está alrededor de `950-1260 ms` en CPU y segmentación alrededor de `1570 ms`,
por lo que depth se ejecuta ahora como experimento controlado, con las demás
ramas deshabilitadas. Esos tiempos no son una medición de depth.

### Implementación Base Realizada

- `InferenceResult` ahora conserva `depth: Option<DepthMap>` y lo extrae desde
  `Results` sin copiarlo.
- Los modelos con `enabled = false` no se cargan ni se ejecutan.
- `depth-standard` está definido como raíz de cascade, con el modelo small/320
  FP16 habilitado y sin crop; detect/pose/face/seg están deshabilitados.
- Depth se excluye de consolidación, tracking, zonas y FSM.
- Las métricas separan píxeles válidos, rango de profundidad y mapas vacíos de
  `infer_empty` basado en detecciones.
- JSONL emite eventos `type=depth` con dimensiones y estadísticas, sin incluir
  la matriz completa.
- Rerun publica el mapa colorizado bajo
  `/world/camera/depth/<model>/<metric|disparity>` y sus estadísticas bajo
  `stats/`.
- Hay tests para serialización de eventos y métricas con píxeles inválidos,
  mapas vacíos y valores finitos.

### Estado De La Prueba Depth-Only

- `config/mana.toml` apunta `default_model` a `depth-standard`.
- Solo `depth-standard` está habilitado; detect, pose, face y seg se omiten al
  cargar.
- Tracking, zonas, FSM, snapshots, frame RGB, boxes y máscaras están apagados.
- La prueba RTSP procesó frames `1920x1080` con `2,073,600` píxeles válidos.
- La latencia observada fue aproximadamente `64-76 ms` de backend y `73-86 ms`
  de pipeline.
- La prueba RTSP con ROI `680x680` procesó `462,400` píxeles válidos y registró
  `roi:[560,140 1240,820]`.

### Probe Reproducible Con Imagen

El binario `depth-image-probe` usa el ONNX original FP32 de `inference`, aplica
el ROI antes de `predict_image`, valida que el mapa vuelva al tamaño completo,
genera el overlay con la misma fórmula de `annotate_image` y puede escribir un
`.rrd` sin depender de que haya un viewer conectado:

```bash
cargo run --bin depth-image-probe -- \
  /home/care/opt/workspace/references/inference/yolo26s-depth.onnx \
  /tmp/opencode/bus-1920x1080.jpg \
  /tmp/opencode/depth-probe-roi-annotated.png \
  --rrd /tmp/opencode/depth-probe-roi.rrd \
  --roi 560 140 1240 820
```

La ejecución produjo un snapshot `1920x1080`, bandas fuera del ROI negras y
`valid_pixels=462400`; el `.rrd` contiene la imagen original, disparity,
overlay anotado, estadísticas y el rectángulo ROI.

## Modelo Mental Del Pipeline

El onboarding describe el flujo:

```text
RTSP -> ingest/decode -> cascade -> infer -> consolidación -> tracking/FSM
                                      |                    |
                                      +--------------------+-> JSONL/Rerun
```

Depth no pertenece al flujo de entidades:

```text
RTSP -> decode -> depth inference -> DepthMap -> métricas/Rerun/JSONL
                         |
                         +-> no Detection, no NMS, no tracking, no FSM
```

Debe ser una salida paralela a las detecciones. La presencia de un mapa válido
no debe crear una detección artificial ni impedir que el consolidator procese
los resultados de `detect`, `pose` o `seg`.

## Qué Ya Soporta El Crate De Inferencia

El crate `/home/care/opt/workspace/references/inference` ya tiene soporte real:

- `Task::Depth` en `src/task.rs`.
- `Results.depth: Option<DepthMap>` en `src/results.rs`.
- `DepthMap.data: Array2<f32>` con valores documentados en metros.
- `DepthMap.orig_shape` con la geometría original.
- `DepthMap.min_depth()` y `max_depth()` sobre píxeles válidos.
- `DepthMap.colorize()` con modos `Metric` y `Disparity`.
- Postprocesamiento de salida `[1,1,H,W]` o `[1,H,W]`.
- Corrección de letterbox y resize bilinear al tamaño original.
- Colocación de mapas dentro de un frame completo cuando el crate conoce un ROI.
- Tests unitarios y E2E ignorados de depth en el propio crate.

El problema actual está en `mana-lite/src/infer.rs`: `collect_detections()` solo
recorre `Results.boxes`. Un modelo depth puede ejecutar correctamente y devolver
`Results.depth`, pero `mana-lite` lo descarta y genera un `InferenceResult` sin
detecciones.

## Diseño Propuesto

### 1. Extender `InferenceResult`

Añadir una salida independiente:

```rust
pub struct InferenceResult {
    pub detections: Vec<Detection>,
    pub depth: Option<DepthMap>,
    // timings and existing fields...
}
```

La propiedad debe ser opcional porque el mismo tipo sigue siendo utilizado por
detect, pose y seg. No introducir `DepthMap` dentro de `Detection`.

En `InferEngine::run()`:

1. Ejecutar `predict_image()` como ahora.
2. Extraer `Results.depth` independientemente de `Results.boxes`.
3. Mantener la extracción de detecciones sin cambios.
4. Devolver ambos resultados en el mismo `InferenceResult`.

El mapa debe moverse desde `Results` para evitar una copia innecesaria. Si la
respuesta trae más de un `Results`, la v1 puede tomar el primer `depth` válido,
igual que el runtime trabaja actualmente con un batch de una imagen.

### 2. Geometría Y ROI

La primera integración debe ejecutar depth sobre frame completo, sin `[crop]`.
Así `DepthMap.data` ya tiene exactamente la geometría del frame y no hay que
resolver offsets adicionales.

Para una futura integración con crop:

- `predict_image()` recibe una imagen recortada.
- El mapa resultante puede estar en la geometría del crop, no en la del frame
  original.
- Hay que colocar el `Array2<f32>` en un buffer full-frame usando `crop_rect` y
  dejar el exterior en cero como inválido.
- Debe existir un test específico para offset, clipping y dimensiones.

No mezclar esta decisión con la primera entrega. Depth completo permite validar
el contrato, Rerun y coste sin introducir simultáneamente un problema espacial.

### 3. Catálogo Y Cascade

Añadir primero una entrada experimental, por ejemplo:

```toml
[models.depth-standard]
enabled = false
path = "models/yolo26s-depth-fp16-320.onnx"
task = "depth"
confidence = 0.0
imgsz = 320
half = true
```

La variante `640` se selecciona cambiando únicamente `path` e `imgsz`:

```toml
path = "models/yolo26s-depth-fp16-640.onnx"
imgsz = 640
```

El modelo no debe ser `default_model`, porque el tracking actual necesita
detecciones. Tampoco debe depender de `detect-fast` en la primera versión: un
depth map de frame completo es una rama root y sus resultados no tienen clase ni
bbox que puedan alimentar `same_frame`.

Regla inicial sugerida:

```toml
[[rules]]
model = "depth-standard"
```

Si el coste resulta demasiado alto, se debe apagar con `enabled = false` o
controlar su frecuencia en el scheduler antes de añadir crops dinámicos.

### 4. Métricas Y Estado `empty`

Hoy `Metrics::tick_inference_model()` recibe exclusivamente `&[Detection]` y
considera vacío cualquier resultado sin detecciones. Eso es incorrecto para un
depth válido.

Separar los conceptos:

- `depth_valid` o `depth_invalid` para el mapa.
- `valid_pixels`.
- `min_depth_m` y `max_depth_m`.
- `depth_empty` solo cuando no existe mapa o no hay píxeles válidos.
- La inferencia depth no debe incrementar `infer_empty` solo porque no tiene
  bounding boxes.

Los valores agregados deben ser métricas, no un array completo en cada evento.

### 5. JSONL

La v1 debe publicar estadísticas, no toda la matriz `H*W` en JSONL:

```json
{
  "type": "depth",
  "frame_id": 123,
  "model": "depth-standard",
  "infer_ms": 180,
  "pipeline_ms": 190,
  "width": 1920,
  "height": 1080,
  "valid_pixels": 1981440,
  "min_depth_m": 0.42,
  "max_depth_m": 8.31
}
```

El mapa completo debe pertenecer a Rerun o a un artefacto debug explícito. Si
algún consumidor necesita la matriz, definir un formato binario/versionado en
otro sprint, no ampliar silenciosamente JSONL.

Cambios previstos:

- Nueva variante `Event::Depth`.
- Constructor en `src/logger/event.rs`.
- Serialización en `src/logger/serialize.rs`.
- Test de campos finitos, mapa sin píxeles válidos y estadísticas normales.

### 6. Rerun

Añadir un toggle explícito en `[viz.send]`:

```toml
depth = true
depth_stats = true
```

Publicar el mapa como una imagen separada, no reemplazar la imagen RGB base:

```text
/world/camera/depth/depth-standard/metric
/world/camera/depth/depth-standard/disparity
/world/camera/depth/depth-standard/stats
```

Primera visualización recomendada:

- Un solo modo configurable, preferiblemente `disparity` para leer cercanía.
- `metric` disponible para validar valores de distancia.
- Píxeles inválidos (`<= 0`) negros o transparentes.
- Dimensiones iguales al frame original.
- Overlay sobre la cámara como fase posterior; inicialmente mantener un panel
  separado para no ocultar cajas/máscaras.

`DepthMap::colorize()` ya contiene la normalización y la paleta, pero
`Colormap`/`DepthViz` no están re-exportados convenientemente por el crate de
inferencia. Hay dos opciones válidas:

1. Re-exportar esos tipos desde `ultralytics-inference`.
2. Añadir un método de alto nivel, por ejemplo `DepthMap::colorize_rgb8(viz)`,
   que devuelva bytes listos para `rerun::Image::from_rgb24`.

Preferir la segunda si se quiere mantener encapsulada la implementación de
visualización del crate.

El blueprint actual se genera principalmente en `src/viz.rs` y no está realmente
dirigido por `rerun.toml`. La primera entrega puede publicar la entidad y dejar
que `auto_views = true` la muestre. Añadir un panel dedicado al blueprint debe
ser una tarea separada después de comprobar el path y las dimensiones.

## Sprint Propuesto

### Fase 0: assets y contrato

- [x] Extender `model-tools promote` para aceptar `depth` como task.
- [x] Promover `yolo26s-depth-fp16-320.onnx`; la variante 640 queda disponible
      como siguiente benchmark.
- [x] Añadir `depth-standard` deshabilitado al catálogo.
- [ ] Validar metadata `task=depth`, shape, dtype FP16, IO y SHA-256.
- [x] Añadir una regla root y confirmar cómo `enabled=false` evita carga,
      scheduling y warmup para este modelo.

**Salida:** artefactos disponibles y una entrada TOML que no cambia el runtime
por defecto.

### Fase 1: runtime interno

- [x] Añadir `depth` a `InferenceResult`.
- [x] Extraer `Results.depth` sin exigir `Results.boxes`.
- [x] Mantener detecciones, NMS, consolidación y tracking sin cambios.
- [x] Implementar mapa frame-completo para la v1.
- [ ] Añadir test sintético con `Results { depth: Some(...), boxes: None }`.
- [x] Añadir test de valores válidos y mapa vacío en métricas/JSONL.

**Salida:** un modelo depth produce un `DepthMap` accesible sin crear entidades.

### Fase 2: métricas y JSONL

- [x] Añadir `Event::Depth` y serializador.
- [x] Añadir estadísticas de profundidad por modelo.
- [x] Corregir la semántica de `empty` para tareas sin detecciones.
- [x] Mantener el payload grande fuera de JSONL.

**Salida:** cada ejecución depth deja una observación pequeña y estable en logs.

### Fase 3: Rerun

- [x] Añadir toggles `depth`, `depth_stats` y `depth_viz`.
- [x] Publicar una imagen RGB colorizada en el path depth.
- [x] Publicar modo metric/disparity de forma explícita.
- [x] Limpiar el path anterior cuando un frame no tiene mapa válido.
- [ ] Añadir panel o vista dedicada después de comprobar el stream.
- [ ] Probar reconexión de Rerun y frames con dimensiones cambiantes.

**Salida:** el mapa es visible en Rerun sin tapar boxes, máscaras ni frame base.

### Fase 4: 320 vs 640 y operación

- [ ] Medir `s/320` primero en CPU.
- [ ] Medir `s/640` después; no asumir que 640 aporta calidad útil sin comparar.
- [ ] Medir `x/320` y `x/640` solo como benchmark de calidad/coste.
- [ ] Comparar latencia, keyframe drops, memoria, `valid_pixels`, rango y
      estabilidad temporal.
- [ ] Elegir una variante activa y dejar la otra comentada en TOML.
- [ ] Documentar frecuencia de ejecución y condición de apagado.

**Salida:** una decisión de despliegue basada en medición, no solo en el nombre
del modelo.

## Riesgos Y Decisiones Pendientes

### Profundidad monocular

Aunque `DepthMap` documenta metros, la profundidad monocular puede no ser
absoluta en todas las escenas. Validar con distancias conocidas antes de usarla
para reglas clínicas o alarmas.

### CPU y FP16

FP16 reduce el modelo y puede ser compatible con ONNX Runtime CPU, pero no se
debe asumir aceleración. Medir el tiempo real del backend; la ejecución de
`xlarge/640` de detección ya mostró que la calidad tiene un coste elevado.

### Memoria

Un mapa `1920x1080` en `f32` ocupa aproximadamente 8 MB, sin contar copias,
colorización y buffers de Rerun. No guardar varios frames depth en memoria.

### ROI

Frame completo es la opción segura para empezar. Depth sobre un crop dinámico
requiere colocar correctamente el mapa de vuelta al frame y marcar el exterior
como inválido. No combinar depth con el crop de persona hasta que exista ese
test geométrico.

### Carga De Modelos Deshabilitados

`InferEngine::from_catalog()` omite entradas con `enabled=false`, por lo que no
se reservan memoria ni se ejecuta warmup para variantes experimentales.
Las entradas habilitadas son obligatorias: si falta su archivo o falla la carga
del ONNX, el proceso termina durante el bootstrap en lugar de arrancar sin
inferencia. El `default_model` también debe estar habilitado.

## Criterios De Aceptación

- `depth-standard` puede activarse solo editando TOML.
- El binario carga el ONNX y detecta `task=depth` sin errores.
- `InferenceResult.depth` existe aunque `detections` sea vacío.
- Depth no crea `Detection`, `ConsolidatedObservation`, track, zona ni transición
  FSM.
- JSONL publica estadísticas finitas y versionables.
- Rerun muestra un mapa con las dimensiones del frame original bajo un path
  dedicado.
- Los frames inválidos no provocan panic ni publican datos anteriores como si
  fueran nuevos.
- La rama depth puede deshabilitarse sin cambiar el comportamiento de detect,
  pose, face o seg.
- Las pruebas existentes de máscaras, consolidación y serialización siguen
  pasando.

## Primera Acción De La Próxima Sesión

1. Activar temporalmente `yolo26s-depth-fp16-320.onnx` con `enabled = true`, sin
   crop.
2. Ejecutar una prueba corta contra la cámara y verificar `type=depth`,
   `valid_pixels`, min/max y dimensiones.
3. Confirmar que el resto del pipeline sigue publicando detecciones normalmente.
4. Comparar después `320` contra `640` y decidir frecuencia/coste operativo.

## Comandos De Verificación

Desde `tools/model-tools`:

```bash
uv run model-tools inspect \
  --input artifacts/yolo26-fp16/yolo26s-depth-fp16-320.onnx
```

Desde la raíz de `mana-lite`:

```bash
cargo check
cargo test infer::tests
cargo test --workspace
git diff --check
```

Las aserciones de configuración fueron alineadas con el catálogo actual
(`confidence = 0.20`, `min_component_area_ratio = 0.05` y
`polygon_simplify = 0.98`).

## Archivos Clave

- `docs/onboarding.md`: modelo mental del pipeline, cascade, ROI y salidas.
- `docs/sprints/seg-standard.md`: patrón de sprint completo para una rama nueva.
- `src/infer.rs`: frontera actual donde se pierde `Results.depth`.
- `src/main.rs`: scheduling, pending outputs, consolidación y registro.
- `src/config.rs`: catálogo y toggles de visualización.
- `src/viz.rs`: publicación actual de frame, ROI, boxes y máscaras.
- `src/logger/event.rs`: tipos de eventos JSONL.
- `src/logger/serialize.rs`: serialización wire.
- `src/metrics.rs`: contadores y semántica actual de `empty`.
- `config/models.toml`: catálogo de modelos.
- `config/cascade.toml`: roots y dependencias.
- `config/viz.toml`: toggles de publicación.
- `config/rerun.toml`: configuración del blueprint.
- `/home/care/opt/workspace/references/inference/src/results.rs`: `DepthMap` y
  colorización.
- `/home/care/opt/workspace/references/inference/src/postprocessing.rs`:
  reconstrucción del mapa depth y geometría de ROI.
