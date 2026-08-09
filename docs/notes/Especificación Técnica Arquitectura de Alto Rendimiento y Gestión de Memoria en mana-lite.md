# Especificación Técnica: Arquitectura de Alto Rendimiento y Gestión de Memoria en mana-lite

### 1. Filosofía de Diseño y Estado del Arte en Rust 2024

El sistema `mana-lite` representa la vanguardia en el desarrollo de sistemas de visión artificial embebidos, aprovechando las capacidades maduras de **Rust Edición 2024** para ofrecer un pipeline de procesamiento que garantiza seguridad de memoria sin sacrificar el determinismo en tiempo real. Al utilizar abstracciones de costo cero (_zero-cost abstractions_) y el motor de concurrencia asíncrona de `tokio`, el sistema logra una ejecución eficiente de múltiples modelos de inferencia sin la sobrecarga de un recolector de basura (Garbage Collector).

Esta arquitectura prioriza un flujo de datos lineal y predecible, lo que resulta en una ventaja competitiva crítica: una latencia de cola (p99) estable. Mientras que los sistemas basados en GC sufren de _jitter_ impredecible debido a las pausas del sistema, `mana-lite` garantiza que cada frame sea procesado en intervalos constantes. Esta filosofía de "seguridad de memoria sin runtime" es fundamental para aplicaciones de misión clínica, donde la estabilidad comienza desde la captura de paquetes crudos en la red mediante una gestión de buffers de bajo nivel.

### 2. Pipeline de Ingesta y Decodificación de Baja Latencia

La eficiencia del sistema depende de una ingesta selectiva y una decodificación ajustada que minimicen el uso de CPU, evitando el fenómeno de "backpressure" en el pipeline.

- **Resiliencia en Ingesta:** El componente `IngestEngine` utiliza `RetinaReader` para gestionar flujos H.264 Annex-B. Para enfrentar condiciones de red inestables, implementa una estrategia de **Error Windowing** (`ErrorWindow`) que monitorea la ratio de errores RTP. Si se supera el umbral, se dispara una reconexión mediante **Exponential Backoff with Jitter**, evitando colisiones tipo "thundering herd" en despliegues masivos.
- **Filtrado y Deduplicación:** El motor descarta automáticamente P-frames, procesando exclusivamente **IDR keyframes**. Además, si un keyframe es idéntico byte a byte al anterior, se suprime para ahorrar ciclos de inferencia redundantes.
- **SoftwareDecoder (FFmpeg):** Configurado para eliminar latencias internas mediante:
    - `LOW_DELAY`: Elimina el buffer de frames internos de FFmpeg, devolviendo el primer frame IDR de inmediato.
    - `threading::Type::None`: Desactiva el multihilo interno para garantizar una latencia predecible y evitar cambios de contexto costosos.

La utilidad `pack_frame_into` es vital aquí: elimina los "strides" (bytes de relleno para alineación SIMD) para producir un buffer denso. Este paso es obligatorio, ya que el backend de `**ultralytics-inference**` requiere un layout contiguo de `width * height * channels` para alimentar los modelos de IA sin errores de segmentación.

### 3. Gestión de Memoria: El Ecosistema BufferPool

En `mana-lite`, el `BufferPool` no es solo una estructura de datos, sino una estrategia para neutralizar el costo de asignación dinámica en el **hot-path** del pipeline. El sistema utiliza un `Mutex<Vec<Vec<u8>>>` que gestiona el ciclo de vida de los buffers: **Acquire** (Ingest) -> **Fill** (Decoder) -> **Release** (Post-process/Viz).

**Impacto Estratégico y de Rendimiento:** | Métrica | Análisis Técnico | | :--- | :--- | | **Ahorro de Memoria** | Un frame 1080p RGB requiere ~6MB. El reuso elimina el _jitter_ del asignador del sistema bajo carga pesada. | | **Control de Backpressure** | La política de acotamiento (`max_bufs`) actúa como mecanismo de seguridad. Si el pool se agota, el sistema descarta frames, previniendo fallos por **Out of Memory (OOM)**. | | **Seguridad en Rutas de Error** | El mecanismo garantiza que los buffers se devuelvan al pool incluso tras fallos en etapas posteriores del procesamiento. |

Este control granular sobre la memoria asegura que los recursos estén disponibles instantáneamente para la ejecución de los modelos de visión en cascada.

### 4. Arquitectura de Inferencia en Cascada y Gestión de ROIs

La inferencia en cascada maximiza la precisión en regiones de interés (ROI) sin procesar innecesariamente el frame completo, optimizando el ancho de banda del bus de datos.

- **Estrategias de ROI:**
    - **Static ROI:** Regiones fijas (ej. una cama) donde el modelo enfoca su atención.
    - **Dynamic Cascade Crops:** Recortes centrados en detecciones previas. Un ejemplo crítico es el modelo `face-yolo`, que utiliza un `**upper_fraction**` **de 0.50** para centrarse exclusivamente en la región superior (cabeza) de una detección de persona.
- **CascadeScheduler:** Orquesta la ejecución mediante modos de gating: `same_frame` (ejecución inmediata sobre el frame actual) o basado en tracks confirmados de frames históricos.
- **Geometría de Transformación:** El sistema traduce coordenadas de "Crop Space" a "Frame Space" mediante offsets `x1`, `y1`, delegando la coherencia matemática a la crate `mana-geometry`.

El post-procesamiento (NMS, `area_ratio`, `confidence`) es la última línea de defensa para filtrar falsos positivos que podrían saturar la lógica de la máquina de estados.

### 5. Optimización de Estructuras de Datos Geométricos

La biblioteca `mana-geometry` normaliza los resultados de inferencia (usando rangos de 0.0 a 1.0) para asegurar la independencia de la resolución del sensor.

|   |   |   |
|---|---|---|
|Estructura / Algoritmo|Implementación Técnica|Propósito|
|**CompactMask**|Almacenamiento **Column-Major** RLE (Run-Length Encoding)|Optimiza la localidad de caché y reduce la huella de memoria en segmentación.|
|**Suzuki-Abe**|Border Following Algorithm|Extrae contornos vectoriales precisos a partir de máscaras rasterizadas.|
|**RDP (Ramer-Douglas-Peucker)**|Simplificación de Polígonos|Reduce la complejidad de los vértices manteniendo la integridad geométrica.|
|**IoU vs IoS**|Intersection over Union / Smaller|IoU para asociación de tracks; IoS para evaluar contención en zonas.|

La transición de máscaras densas a `CompactMask` es fundamental para minimizar el impacto en el ancho de banda del bus de datos cuando los resultados se transmiten a la capa lógica de toma de decisiones.

### 6. Lógica de Estado y Estabilidad del Sistema (FSM & Health)

La resiliencia ante condiciones adversas se gestiona mediante la FSM y el monitor de salud, validados rigurosamente durante la fase de `**bootstrap**` en `validation.rs` para evitar estados inconsistentes en producción.

- **Hysteresis y Dualidad Temporal:**
    - **PresenceFilter:** Utiliza un conteo basado en **inference ticks** para debouncing de señales de corta duración.
    - **OccupancyStateMachine:** Utiliza `**std::time::Instant**` para aplicar una histéresis basada en tiempo real (milisegundos), garantizando que los cambios de estado (ej. de `Single` a `Empty`) sean persistentes.
- **Wildcard Transitions:** El sistema permite transiciones comodín (ej. desde `*`) para manejar errores críticos como `data_stale` (datos obsoletos) o `blind` (pérdida de señal de cámara).
- **Integridad del Pipeline:** El `MetricsEngine` detecta ciclos de estancamiento, permitiendo que el sistema se recupere automáticamente ante fallos en el flujo de frames.

### 7. Observabilidad y Diagnóstico de Rendimiento

El stack de observabilidad está diseñado para ser no intrusivo, desacoplando el `LogManager` de los `LogHandlers` (JSONL, Metrics, Rerun).

- **Serialización de Alto Rendimiento:** En el "hot-path" del logger, `serialize.rs` evita serializadores genéricos como `serde_json`. En su lugar, utiliza un **serializador manual** que escribe bytes crudos directamente al buffer, eliminando la creación de Representaciones Intermedias (IR) y minimizando las asignaciones de memoria.
- **VizBridge y Rerun.io:** Actúa como puente hacia el motor de visualización, publicando datos en **múltiples líneas de tiempo** (`frame_nr` y `frame_time`). Esto permite una inspección sincronizada donde los ingenieros pueden correlacionar exactamente un frame de video con las detecciones, las máscaras segmentadas y el estado actual de la FSM.
- **Métricas de Calidad:** Se reportan latencias de inferencia por modelo, _drops_ de ingesta y _throughput_ de decodificación, cerrando el ciclo de optimización continua del sistema.
