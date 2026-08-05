**Arbol de metricas de inferencia en Rerun**

Las metricas de inferencia se organizan por modelo/task, no de forma plana. Cada modelo tiene 3 ramas: `active` (throughput), `warnings` (anomalias) y `detections` (calidad de detecciones).

Ademas, los bounding boxes de cada deteccion se renderizan como overlays en la vista `Camera` bajo `/world/camera/detections/{model}/{clase}/{i}`.

### Arbol de paths (per-model)

```
/infer/{model}/
├── active/
│   ├── hz              ─ inferencias/segundo de este modelo
│   ├── avg_ms          ─ latencia promedio de inferencia
│   ├── min_ms          ─ latencia minima de la ventana
│   ├── max_ms          ─ latencia maxima de la ventana
│   └── yield_avg       ─ detecciones promedio por inferencia
├── warnings/
│   ├── skips           ─ veces que el cascade salteo este modelo
│   └── empty           ─ inferencias con cero detecciones
├── detections/
│   ├── conf_avg        ─ confianza promedio de las detecciones
│   ├── conf_min        ─ confianza minima (peor deteccion de la ventana)
│   └── area_avg        ─ area promedio de los bounding boxes
├── classes/{class}     ─ conteo acumulado por clase en la ventana
└── per_frame/          ★ datos per-frame (no promediados)
    ├── counts/{class}  ─ cuantas detecciones de esta clase en este frame
    ├── conf/{class}/
    │   ├── min         ─ confianza minima de esta clase en este frame
    │   └── max         ─ confianza maxima de esta clase en este frame
    └── area/{class}/
        ├── min         ─ area minima de bbox de esta clase en este frame
        └── max         ─ area maxima de bbox de esta clase en este frame

/world/camera/detections/{model}/{class}/{i}/
└── Boxes2D con label "{class} {confidence}" ─ overlay en Spatial2DView
```

### Periodo de emision

| Metrica | Frecuencia | Path |
|---------|-----------|------|
| Per-frame (latency per model) | Cada keyframe | `/pipeline/infer/{model}/latency_us` |
| Per-frame (bboxes) | Cada keyframe | `/world/camera/detections/**` |
| Per-frame (class counts, conf, area) | Cada keyframe | `/infer/{model}/per_frame/**` |
| Per-window (active, warnings, detections quality) | Cada `report_interval_s` (default 5s) | `/infer/{model}/active/**`, `warnings/**`, `detections/**` |
| Per-window (class counts acumulados) | Cada `report_interval_s` | `/infer/{model}/classes/{class}` |

Los bboxes se limpian por modelo en cada frame — cada modelo tiene su propio sub-arbol, asi que los boxes de `detect-fast` y `pose-standard` coexisten.

### Per-frame class stats — uso practico

Los datos per-frame bajo `/infer/{model}/per_frame/` te permiten ver la variabilidad frame a frame, no promediada:

- **`counts/{class}`**: picos o silencios de deteccion. Si `person` pasa de 3 a 0 de golpe, la camara se tapo o el modelo perdio la clase.
- **`conf/{class}/min` y `max`**: estabilidad de la confianza del modelo. Si min y max divergen mucho en un mismo frame, hay detecciones de la misma clase con confianza muy variable → escena ruidosa.
- **`area/{class}/min` y `max`**: consistencia del tamano de bbox. Si el area de `person` crece > 3x entre frames, la persona se acerco mucho a la camara — posible evento clinico.

### Diagnostico con per-frame stats

**El conteo de personas oscila entre 0 y 2 cada pocos frames:**
→ El modelo esta inseguro. Mira `conf/{person}/min` — si baja de 0.3, subi el threshold de confianza en `models.toml`.

**Hay 5 detecciones de person en un frame y 0 en el siguiente:**
→ Falso positivo masivo o el modelo vio patrones en ruido. El `area_max` de ese frame probablemente sea muy chico (detecciones diminutas = ruido).

**El area de bed es constante pero person varia 10x:**
→ La cama es estatica (bueno). La persona se mueve hacia/desde la camara (esperado).

### Interpretacion

**Active** — throughput del modelo

- `hz` > 0 → el modelo esta corriendo. Si es 0, el cascade lo esta saltando o el modelo no esta en la lista de modelos activos.
- `avg_ms` < 50ms → ok. Si un modelo hijo (ej. `pose-standard`) es 5x mas lento que el padre (`detect-fast`), considerar si vale la pena correrlo en cada frame.
- `yield_avg` ≈ 1 → tipico para modelos de deteccion general. Si es > 3, el modelo esta viendo muchos objetos (escena ruidosa o confianza muy baja).

**Warnings** — si hay que intervenir

- `skips` > 0 persistente → el cascade no esta satisfaciendo la dependencia. El modelo padre no detecto la clase requerida. Verificar confianza del padre o si la clase esperada esta presente en la escena.
- `empty` = `inferences` → el modelo corre pero nunca produce detecciones. Confianza demasiado alta o modelo incorrecto para la escena.

**Detections** — calidad de lo que el modelo ve

- `conf_avg` > 0.5 → ok. Si baja de 0.4, el modelo esta inseguro — ajustar threshold o revisar escena.
- `conf_min` → la deteccion mas debil de la ventana. Sirve para calibrar el parametro `confidence` del modelo.
- `area_avg` → tamano tipico de los objetos detectados. Si cambia bruscamente, la camara pudo haberse movido o cambiado la escena.

**Camera overlay** — dato puro

- Los bboxes se renderizan sobre la imagen en el panel `Camera`. Cada modelo tiene color independiente asignado por Rerun.
- El label muestra `"{clase} {confianza}"` (ej. `person 0.87`).
- Si no ves cajas en la camara pero `yield_avg > 0`, verificar que Rerun este recibiendo los datos (puede ser un tema de timeline o entidad).

### Ejemplo de log

```
ingest: 0.4 Hz — 2 keyframes in 5s | decode 10ms avg | cycles 64 | pframes:72 timeouts:64
infer:  0.4 Hz — 2 calls in 5s | 38ms avg | 2 dets | skips:1 empty:0
```

### Ejemplo de Rerun en runtime

```
/infer/detect_fast/active/hz           = 0.4
/infer/detect_fast/active/avg_ms       = 38
/infer/detect_fast/active/yield_avg    = 1.0
/infer/detect_fast/warnings/skips      = 0
/infer/detect_fast/warnings/empty      = 0
/infer/detect_fast/detections/conf_avg = 0.81
/infer/detect_fast/detections/conf_min = 0.70
/infer/detect_fast/detections/area_avg = 489000

/infer/pose_standard/active/hz         = 0.0      ← nunca corre (no hay persona)
/infer/pose_standard/warnings/skips    = 2        ← cascade lo saltea
```

Esto dice: `detect-fast` funciona bien — 0.4 Hz, 38ms, confianza 0.81. `pose-standard` nunca corre porque `detect-fast` no detecta `person` en esta escena (solo detecta `bed`).
