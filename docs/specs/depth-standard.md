# Blueprint y Spec-004 - Rama `depth-standard`

**Estado:** Baseline operativo ROI-local  
**Version:** 0.1  
**Fecha:** 2026-08-07  
**Responsable funcional:** Mana Lite Vision  
**Audiencia:** administradores, operadores, funcionales y desarrolladores  

Este documento es el artefacto maestro para continuar la integracion de
profundidad monocular YOLO26. Define el comportamiento esperado, la
configuracion, los contratos de datos, la operacion y el plan de evolucion.

## 1. Objetivo De Negocio

`depth-standard` estima profundidad dentro de una region de interes de la
camara. Su salida sirve para comprender distancia y construir reglas
funcionales posteriores, por ejemplo:

- distancia de una persona a una cama, silla o limite de seguridad;
- deteccion de aproximacion o alejamiento;
- medicion de profundidad en una subregion de una deteccion;
- diagnostico visual de la escena en Rerun.

Depth no crea personas, no asigna identidad, no crea tracks y no debe disparar
otros modelos. Es una fuente de evidencia numerica y visual.

## 2. Alcance Y No Alcance

### Dentro Del Alcance

- Cargar modelos YOLO26 depth en formato ONNX.
- Ejecutar depth sobre un ROI fijo de la camara.
- Publicar estadisticas validas en JSONL y metricas.
- Mostrar BGR, ROI y disparity en Rerun.
- Consultar profundidad de regiones globales mediante coordenadas locales del
  mapa depth.
- Comparar variantes `small`, `medium`, `large` y `xlarge` en `320` y `640`.

### Fuera Del Alcance

- Convertir profundidad monocular en medicion calibrada de sensor.
- Usar depth como padre de `face-yolo` o `seg-standard`.
- Enviar la matriz completa de profundidad a JSONL.
- Crear reglas clinicas sin calibracion de escena y validacion funcional.

## 3. Roles Y Responsabilidades

| Rol | Responsabilidad | Evidencia esperada |
|---|---|---|
| Administrador | Paths, modelo activo, transporte RTSP y permisos | `models.toml`, logs de arranque |
| Operador | Arranque, Rerun, lectura de latencia y salud | terminal, Rerun, JSONL |
| Funcional | Define regiones, umbrales y significado de distancia | casos de uso, tabla de reglas |
| Desarrollador | Cambia contratos, postprocess, ROI y visualizacion | tests, ADR, commit |
| Integrador | Promueve ONNX y registra digest | artifact, SHA-256, catalogo |

Ningun rol debe modificar credenciales reales en archivos versionados.

## 4. Modelo Mental SAP Blueprint

```text
Solicitud funcional
    |
    v
Caso de uso de profundidad
    |
    +--> Region global de interes
    |        |
    |        v
    |    Interseccion con ROI depth
    |        |
    |        v
    |    Coordenadas locales del mapa
    |
    +--> Modelo ONNX + tamano de entrada
    |
    v
Configuracion aprobada
    |
    v
Prueba imagen -> Prueba RTSP -> Evidencia Rerun/JSONL
    |
    v
Promocion o rollback
```

La configuracion es el objeto de control. El modelo no se considera operativo
solo porque el archivo ONNX exista: debe cargar, inferir, publicar estadisticas
y pasar la prueba de ROI.

## 5. Arquitectura Tecnica

```text
RTSP
  -> keyframe
  -> decode RGB 1920x1080
  -> cascade roots
       +-> detect-fast
       |     +-> face-yolo (same_frame, hijo)
       |     +-> seg-standard (same_frame, hijo)
       |
       +-> depth-standard (root independiente)
              -> mapa depth del ROI
              -> metricas y JSONL
              -> overlay opcional Rerun
```

Reglas importantes:

1. `depth-standard` es una raiz de cascada (`requires` ausente).
2. `face-yolo` y `seg-standard` dependen de `detect-fast`, no de depth.
3. Depth no entra en `DetectionConsolidator` ni en `Tracker`.
4. `0 dets` no significa depth invalido; depth se valida con `valid_pixels`.
5. Un modelo habilitado que falta o no carga detiene el bootstrap.

## 6. Catalogo De Modelos

Configuracion base:

```toml
[models.depth-standard]
enabled = true
path = "models/yolo26l-depth-fp16-320.onnx"
task = "depth"
confidence = 0.0
imgsz = 320
half = true

[models.depth-standard.crop]
type = "static"
region = [560, 140, 1240, 820]
```

El ROI actual mide `680x680` y tiene `462400` posiciones. Las variantes
exportadas se encuentran en:

```text
tools/model-tools/artifacts/depth-fp16/
```

La matriz validada incluye:

```text
yolo26m-depth-fp16-{320,640}.onnx
yolo26l-depth-fp16-{320,640}.onnx
yolo26x-depth-fp16-{320,640}.onnx
```

### Benchmark Medido (2026-08-07)

Medido con `depth-image-probe` (CPU, FP16, `--threads 0`, ROI `[560,140 1240,820]`,
warmup 1 + 3 repeticiones) sobre dos escenas: `bus-1920x1080.jpg` (imagen fija
canonica) y un frame real `1920x1080` del clip de camara `clip1_minuto.mp4`.

| Variante | Latencia bus | Latencia escena real | Rango bus (m) | Rango escena (m) | Nota |
|---|---|---|---|---|---|
| s/320 | 80 ms | 70 ms | 1.64-6.53 | 0.82-2.70 | barata, estructura aceptable en escena real |
| m/320 | 139 ms | 154 ms | 2.54-11.0 | 2.12-5.58 | buena estructura, rango metrico sobrestimado |
| l/320 | 166 ms | 172 ms | 2.75-7.07 | 1.96-3.44 | **baseline runtime** (bajo y consistente) |
| x/320 | 279 ms | 328 ms | 1.09-10.7 | 2.23-5.25 | **outlier estructural — no recomendado** |
| s/640 | 268 ms | 236 ms | 1.03-2.65 | 0.59-1.49 | rango metrico mas plausible |
| m/640 | 496 ms | 492 ms | 1.26-5.29 | 0.77-2.15 | opcion de calidad, rango plausible |
| l/640 | 584 ms | 631 ms | 1.29-3.89 | 0.96-2.48 | calidad, coste alto |
| x/640 | 1110 ms | 1170 ms | 1.16-5.16 | 0.80-1.86 | maxima calidad, no viable 24/7 en CPU |

Observaciones:

1. **x/320 es un outlier estructural**: su mapa correlaciona 0.41-0.64 con
   todas las demas variantes en ambas escenas (mientras s/m/l y la familia 640
   correlacionan 0.8-0.99 entre si). El baseline anterior (x/320) era la
   variante con la estructura menos representativa de la familia.
2. **La familia 320 sobrestima el rango metrico** (max depth ~2x respecto a
   640 en la misma escena). Para reglas clinicas de distancia, `640` da rangos
   mas plausibles; la calibracion de escena sigue siendo obligatoria (no
   afirmar distancia metrica sin referencia fisica).
3. **l/320 es el compromiso elegido**: 172 ms en escena real (2x mas rapido
   que el x/320 anterior), estructura consistente con la familia, rango
   metrico mas ajustado que m/320.
4. **m/640 es la opcion de calidad** si el presupuesto de latencia lo permite
   (492 ms: evaluar contra el GOP de la camara y el ciclo completo del
   pipeline antes de activarla).

### Seleccion De Variante

| Variante | Uso | Decision |
|---|---|---|
| s/320 | presupuesto minimo | solo si la latencia es critica |
| m/320 | comparacion de calidad/coste | laboratorio |
| l/320 | baseline runtime (2026-08-07) | preferida para desarrollo |
| m/640 | calidad metrica | probar si el presupuesto de latencia lo permite |
| x/320 | — | no usar: outlier estructural medido |
| * / 640 | mayor detalle de entrada | solo si la latencia lo permite |

Cambiar el modelo requiere cambiar `path` e `imgsz` en el mismo bloque y
registrar el resultado de la prueba. No se deben sobrescribir baselines sin
digest SHA-256.

## 7. ROI Y Sistema De Coordenadas

El frame de camara usa coordenadas globales. El mapa depth debe usar
coordenadas locales al ROI.

```text
Frame global: 1920x1080
Depth ROI:    [ox, oy, x2, y2] = [560, 140, 1240, 820]
Mapa local:   680x680
```

Para consultar una region global `[gx1, gy1, gx2, gy2]`:

```text
ix1 = max(gx1, ox)
iy1 = max(gy1, oy)
ix2 = min(gx2, x2)
iy2 = min(gy2, y2)

local = [ix1 - ox, iy1 - oy, ix2 - ox, iy2 - oy]
```

Si `ix2 <= ix1` o `iy2 <= iy1`, la region no tiene profundidad disponible.

Ejemplo:

```text
Region global: [768, 320, 900, 500]
Region local:  [208, 180, 340, 360]
```

No se deben consultar pixeles del frame completo para una regla depth. La
regla debe pedir una region y recibir estadisticas robustas sobre el mapa local.

## 8. Contrato De Profundidad

La salida conceptual de depth es:

```text
DepthRoiMap {
  data: Array2<f32>,
  roi: [u32; 4],
  map_width: u32,
  map_height: u32,
}
```

Un valor es valido si es finito y mayor que cero. Las reglas no deben basarse
en un unico pixel. La consulta recomendada es:

```text
DepthRegionStats {
  valid_pixels,
  valid_ratio,
  min_depth_m,
  median_depth_m,
  p10_depth_m,
  p90_depth_m,
}
```

### Estado Actual Y Evolucion

| Capacidad | Estado |
|---|---|
| Inferencia solo sobre ROI | implementada |
| Estadisticas validas y eventos JSONL versionados (v2) | implementada |
| Consultas por region con mediana/p10/p90 (§7 global->local) | implementada |
| Depth como raiz independiente | implementada |
| Mapa materializado en frame completo | eliminado para ROI activo |
| Consulta ROI-local sin expansion | baseline aprobado |
| Overlay RGBA transparente fuera del ROI | visualizacion opcional |

La expansion a `1920x1080` no agrega inferencia, pero multiplica memoria,
colorizacion y ancho de banda visual por aproximadamente `4.5`. Debe ser una
operacion exclusiva de visualizacion cuando Rerun esta habilitado.

### Consulta De Region

El contrato interno `DepthRoiMap` y las consultas `DepthRegionStats` viven en
`src/depth.rs` (`mana_lite::depth`), compartidos por el runtime y por el probe:

```text
region_intersection(roi, region) -> Option<[u32; 4]>   # §7 global->local
region_stats(depth, roi, region)  -> Option<DepthRoiStats>
map_dims(depth)                   -> (width, height)
```

`region_stats` devuelve `None` si la region no interseca el ROI o no hay
ningun valor valido. El probe expone la consulta con `--region x1 y1 x2 y2`.

## 9. Cascada Y Reglas

La regla depth es deliberadamente una raiz:

```toml
[[rules]]
model = "detect-fast"

[[rules]]
model = "face-yolo"
requires = "detect-fast"
requires_class = "person"
same_frame = true

[[rules]]
model = "seg-standard"
requires = "detect-fast"
requires_class = "person"
same_frame = true

[[rules]]
model = "depth-standard"
```

Depth puede ejecutarse aunque no haya detecciones. Las reglas funcionales
(`DepthRegionRule`) consumen `DepthRegionStats` y emiten evidencia numerica;
no se convierten en una dependencia de modelo.

### Reglas Funcionales (`DepthRegionRule`)

Definidas en `config/depth-rules.toml` (referenciado desde `mana.toml`
`[inference].depth_rules_file`). Una regla consulta una region global contra
el mapa local del ROI y compara una metrica robusta con un umbral:

```toml
[[rules]]
name = "bed-approach"
region = [560, 140, 1240, 820]
metric = "median"   # min | median | p10 | p90 | max
op = "lt"           # lt | gt
threshold_m = 1.5
min_valid_ratio = 0.5
# Opcional, despues de medir una referencia fisica en esta escena:
# calibration = { reference_model_m = 2.0, reference_scene_m = 1.0 }
```

Reglas:

1. Emiten el evento `depth_region` con evidencia numerica (`value`,
   `threshold_m`, `triggered`, `valid_pixels`, `valid_ratio`).
2. No evaluan si la region no interseca el ROI o no alcanza
   `min_valid_ratio` de valores validos.
3. **No gatean** face, segmentacion ni ningun otro modelo.
4. Los umbrales son provisionales hasta calibrar por camara y escena.
5. `calibration` fija la escala con una referencia fisica de un punto:
   `scene_value = model_value * reference_scene_m / reference_model_m`.
   Con calibracion, `value` y `threshold_m` son unidades de escena; sin ella
   permanecen en unidades relativas del modelo.
6. Una calibracion solo puede afirmarse despues de medir la referencia fisica;
   la salida sin referencia no debe presentarse como distancia metrica.

El resultado de una regla valida alimenta el snapshot del frame para guards FSM
`{ type = "depth_rule", rule = "..." }`. El guard no se cumple si la regla no
tuvo evidencia en el frame. Asi se combinan profundidad y zonas dentro de la
FSM sin hacer que depth gatee face o segmentacion.

Implementacion: `src/depth.rs` (`DepthRules::validate`, `DepthRegionRule::evaluate`),
compartida por runtime y probe.

## 10. Publicacion JSONL

El evento conserva estadisticas, no la matriz completa. Es versionado
(`version: 2`): agrega `roi`, `map_width`, `map_height` y `valid_ratio` al
esquema original (contrato `DepthRoiMap`):

```json
{
  "type": "depth",
  "version": 2,
  "frame_id": 123,
  "model": "depth-standard",
  "infer_ms": 172,
  "pipeline_ms": 185,
  "roi": [560, 140, 1240, 820],
  "map_width": 680,
  "map_height": 680,
  "valid_pixels": 462400,
  "valid_ratio": 1.0,
  "min_depth_m": 1.96,
  "max_depth_m": 3.44
}
```

El evento debe interpretarse en el espacio local del ROI: el campo `roi`
vuelve a ser las coordenadas globales del mapa (inferidas del crop del
modelo) y `map_width`/`map_height` las dimensiones del mapa local. Con eso se
puede transformar cualquier coordenada global a local (interseccion §7) sin
reconstruir el frame completo.

### Evento De Regla (`depth_region`)

Cada regla con evidencia valida emite un evento versionado (`version: 2`):

```json
{
  "type": "depth_region",
  "version": 2,
  "frame_id": 123,
  "rule": "bed-approach",
  "region": [560, 140, 1240, 820],
  "metric": "median",
  "value": 1.42,
  "threshold_m": 1.5,
  "triggered": true,
  "valid_pixels": 23760,
  "valid_ratio": 1.0,
  "calibration": {
    "reference_model_m": 2.0,
    "reference_scene_m": 1.0
  }
}
```

`value` es la metrica de la region (nula si no hay profundidad valida),
`triggered` indica si la comparacion con el umbral se cumple. `calibration` es
`null` cuando la regla usa unidades relativas del modelo. La regla no depende
de detecciones y no activa otros modelos; solo expone evidencia al snapshot
FSM del mismo frame.

## 11. Rerun Y Blueprint Visual

Rutas esperadas:

```text
/world/camera/bgr
/world/camera/rois/depth-standard
/world/camera/crops/depth-standard/depth/disparity
/world/camera/depth/depth-standard/stats/valid_pixels
/world/camera/detections/detect-fast
/world/camera/observations
```

Politica visual:

- BGR es la capa de contexto.
- Disparity es un overlay RGBA.
- Dentro del ROI el alpha es parcial para conservar el frame visible.
- Fuera del ROI el alpha es `0`, nunca un relleno negro que oculte la escena.
- El mapa completo para Rerun es opcional y no debe ser el contrato interno de
  las reglas.

Si Rerun muestra solo crops, revisar primero `config/viz.toml`:

```toml
[viz.send]
frames = true
boxes = true
crop_frames = true
roi_rects = true
```

Tambien se debe limpiar o reabrir la sesion del viewer para evitar entidades
historicas de una ejecucion anterior.

## 12. Operacion Diaria

### Arranque

```bash
cargo run --bin mana-lite -- --config config/mana.toml
```

El arranque correcto debe mostrar:

```text
model detect-fast: loaded (detect)
model depth-standard: loaded (depth)
inference: N model(s) loaded
```

Un ONNX habilitado faltante debe producir error y terminar el proceso. Nunca
debe continuar con `0 model(s) loaded`.

### Lectura De Salud

Para depth, observar:

```text
depth-standard: ... | roi:[560,140 1240,820]
```

Y en JSONL:

- `valid_pixels == 462400` para el ROI completo;
- `infer_ms` y `pipeline_ms` dentro del presupuesto;
- `min_depth_m` y `max_depth_m` finitos;
- ausencia de `depth` cuando el modelo no pudo producir mapa.

### Diagnostico Basico

```bash
uv run model-tools inspect --input models/yolo26l-depth-fp16-320.onnx
cargo test --workspace
cargo run --bin depth-image-probe -- \
  models/yolo26l-depth-fp16-320.onnx \
  ../inference/runs/depth/predict8/bus-1920x1080.jpg \
  /tmp/depth-l-320.png \
  --roi 560 140 1240 820 \
  --imgsz 320 --half \
  --rrd /tmp/depth-l-320.rrd
```

Para medir latencia estable (benchmark de variantes) agregar `--warmup N
--repeats N`; el probe imprime `load` y `latency=mean/min/max` por ejecucion.

El probe debe confirmar mapa, ROI, `valid_pixels` y snapshot antes de cambiar
el runtime RTSP.

## 13. Casos De Prueba Y Aceptacion

| ID | Caso | Resultado esperado |
|---|---|---|
| DEP-001 | ONNX habilitado existe | arranque y `loaded (depth)` |
| DEP-002 | ONNX habilitado falta | proceso termina durante bootstrap |
| DEP-003 | ROI completo valido | `valid_pixels = 462400` |
| DEP-004 | Region global dentro del ROI | consulta local con estadisticas validas |
| DEP-005 | Region parcialmente fuera | interseccion y `valid_ratio` correcto |
| DEP-006 | Region sin interseccion | resultado sin profundidad valida |
| DEP-007 | Depth sin detecciones | evento depth valido, sin `detection` artificial |
| DEP-008 | Face/seg habilitados | corren solo despues de `detect-fast` |
| DEP-009 | Rerun activo | BGR visible, overlay parcial y exterior transparente |
| DEP-010 | Rerun apagado | no se materializa buffer visual innecesario |
| DEP-011 | Modelo cambia de tamano | path, `imgsz`, digest y latencia registrados |
| DEP-012 | Regla con region dentro del ROI | evento `depth_region` con `value` y `triggered` correctos |
| DEP-013 | Regla con region fuera del ROI | sin evento `depth_region` para esa regla |
| DEP-014 | Regla con `min_valid_ratio` no alcanzado | sin evento `depth_region` para esa regla |
| DEP-015 | Regla duplicada o umbral invalido | bootstrap rechaza `depth-rules.toml` |
| DEP-016 | Regla calibrada con referencia valida | `value` se escala antes de comparar y el evento conserva la referencia |
| DEP-017 | FSM usa `depth_rule` sin evidencia | guard falso; con evidencia combina depth + zona y puede transicionar |

## 14. Promocion Y Rollback

### Promocion

1. Exportar desde checkpoint original con `--half`.
2. Inspeccionar shape, opset y dtypes.
3. Ejecutar probe sobre imagen fija.
4. Ejecutar prueba RTSP controlada.
5. Registrar latencia, rango, `valid_pixels` y SHA-256.
6. Promover el archivo al directorio `models/`.
7. Cambiar `path` e `imgsz` juntos.
8. Reiniciar y validar logs de carga.

### Rollback

Volver al ultimo `path` aprobado, restaurar `imgsz`, reiniciar y comparar el
evento `meta.model_loaded`. El rollback no debe cambiar la definicion de ROI ni
las reglas funcionales sin una solicitud separada.

## 15. Roadmap Controlado

### Implementado En Esta Iteracion

- Mantener `DepthMap.data` local al ROI.
- Publicar disparity y annotated como crop independiente en Rerun.
- Mantener BGR completo como contexto separado.

### Proximo Incremento

- Evitar materializar full-frame depth incluso en el adaptador Rerun.

### Siguiente Etapa

- Validar referencias fisicas y promover perfiles calibrados por camara/escena.
- Medir casos clinicos de distancia a borde y aproximacion/alejamiento con video real.

### Implementado En La Etapa 5

- `DepthCalibration` de un punto con escala explicita y trazabilidad en
  `depth_region` v2; sin referencia, la salida sigue siendo relativa al modelo.
- Guard FSM `depth_rule`, con snapshot por frame y validacion de nombres contra
  `config/depth-rules.toml`; ausencia de evidencia no dispara transiciones.
- Ejemplo `watching -> bed_approaching` combina ocupacion de zona bed y la regla
  `bed-approach`, sin gatear face ni segmentacion.

### No Hacer Aun

- No hacer que depth gatee face o segmentation.
- No usar un pixel individual como alarma.
- No afirmar distancia metrica calibrada sin referencia fisica.
- No aumentar a `640` en produccion sin presupuesto de latencia.

## 16. Trazabilidad

| Tema | Fuente |
|---|---|
| Integracion depth | `docs/adrs/024-depth-roi-local.md` |
| Catalogo y cascada | `config/models.toml`, `config/cascade.toml` |
| Pipeline | `src/infer.rs`, `src/main.rs` |
| Metricas y JSONL | `src/metrics.rs`, `src/logger/event.rs` |
| Rerun | `src/viz.rs`, `config/viz.toml` |
| Exportacion | `tools/model-tools/README.md` |
| Operacion general | `docs/operations.md` |

Este documento debe actualizarse cuando cambie el contrato de coordenadas, el
espacio del mapa, el formato JSONL o la politica de cascada.
