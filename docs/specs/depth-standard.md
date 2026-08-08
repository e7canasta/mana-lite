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
path = "models/yolo26x-depth-fp16-320.onnx"
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

La variante small/320 es el baseline funcional. La variante xlarge/320 fue
cargada correctamente en runtime, con una latencia aproximada de `250-350 ms`
en CPU durante la prueba realizada.

### Seleccion De Variante

| Variante | Uso | Decision |
|---|---|---|
| small/320 | baseline y calibracion rapida | preferida para desarrollo |
| medium/320 | comparacion de calidad/coste | laboratorio |
| large/320 | mayor capacidad dentro del ROI | laboratorio |
| xlarge/320 | calidad maxima con coste alto | prueba controlada |
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
| Estadisticas validas y eventos JSONL | implementada |
| Depth como raiz independiente | implementada |
| Mapa materializado en frame completo | eliminado para ROI activo |
| Consulta ROI-local sin expansion | baseline aprobado |
| Overlay RGBA transparente fuera del ROI | visualizacion opcional |

La expansion a `1920x1080` no agrega inferencia, pero multiplica memoria,
colorizacion y ancho de banda visual por aproximadamente `4.5`. Debe ser una
operacion exclusiva de visualizacion cuando Rerun esta habilitado.

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

Depth puede ejecutarse aunque no haya detecciones. En el futuro, una regla
funcional puede consumir `DepthRegionStats`, pero no debe convertir esa
informacion en una dependencia de modelo.

## 10. Publicacion JSONL

El evento actual conserva estadisticas, no la matriz completa:

```json
{
  "type": "depth",
  "frame_id": 123,
  "model": "depth-standard",
  "infer_ms": 268,
  "pipeline_ms": 281,
  "width": 680,
  "height": 680,
  "valid_pixels": 462400,
  "min_depth_m": 2.19,
  "max_depth_m": 5.16
}
```

El evento debe interpretarse en el espacio local del ROI:

```json
{
  "map_space": "roi",
  "roi": [560, 140, 1240, 820],
  "map_width": 680,
  "map_height": 680,
  "valid_ratio": 1.0
}
```

El `roi` sigue viajando por la metrica del modelo y permite transformar
coordenadas globales a locales. No se debe reconstruir el frame completo para
consumir este evento.

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
uv run model-tools inspect --input models/yolo26x-depth-fp16-320.onnx
cargo test --workspace
cargo run --bin depth-image-probe -- \
  models/yolo26x-depth-fp16-320.onnx \
  ../inference/runs/depth/predict8/bus-1920x1080.jpg \
  /tmp/depth-x-320.png \
  --roi 560 140 1240 820 \
  --rrd /tmp/depth-x-320.rrd
```

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

- Introducir `DepthRoiMap` como nombre explicito del contrato interno.
- Agregar `roi`, `map_width`, `map_height` y `valid_ratio` al evento versionado.
- Implementar consultas por region con mediana y percentiles.
- Evitar materializar full-frame depth incluso en el adaptador Rerun.

### Siguiente Etapa

- Definir `DepthRegionRule` sin dependencia de modelos.
- Calibrar umbrales por camara y escena.
- Persistir perfiles funcionales de regiones.
- Integrar reglas depth con zonas/FSM sin mezclar identidad y percepcion.

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
