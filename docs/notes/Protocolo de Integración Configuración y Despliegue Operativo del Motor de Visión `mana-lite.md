# Protocolo de Integración: Configuración y Despliegue Operativo del Motor de Visión `mana-lite`

Este protocolo define los estándares técnicos para la implementación, configuración y despliegue del sistema `mana-lite`. Como motor de visión de alto rendimiento, su diseño está orientado a la baja latencia extrema en entornos clínicos, facilitando la detección de presencia y eventos conductuales específicos (ej. "en cama", "saliendo") mediante una arquitectura de procesamiento lineal y determinista.

## 1. Arquitectura del Sistema y Flujo de Datos Lineal

El sistema `mana-lite` se estructura como un pipeline síncrono orquestado por la `App` struct. Esta linealidad es una decisión arquitectónica estratégica: en el monitoreo clínico, cada milisegundo de latencia acumulada degrada la utilidad de las alertas de seguridad. El pipeline transforma paquetes RTSP en estados lógicos mediante un flujo unidireccional que minimiza los cambios de contexto y la contención de memoria.

### Análisis del Pipeline y Ciclo de Vida del Frame

El ciclo de vida de cada unidad de procesamiento atraviesa cinco etapas críticas:

1. **Ingesta (**`**IngestEngine**`**):** Captura flujos H.264 Annex-B mediante `RetinaReader`, priorizando siempre la "información más fresca" y descartando buffers acumulados para evitar el lag acumulativo.
2. **Inferencia (**`**InferEngine**`**):** Ejecución de modelos YOLO (detección, segmentación, pose) definidos en el `ModelCatalog`.
3. **Tracking y Lógica Espacial:** Transformación de detecciones discretas en entidades temporales persistentes mediante filtros de Kalman y evaluación contra el `ZoneEngine`.
4. **Evaluación de Estado:** Procesamiento de señales por el `OccupancyStateMachine` y el `FsmEngine` para determinar transiciones lógicas complejas.
5. **Observabilidad:** Despacho de telemetría hacia el `LogManager` y visualización espacial vía `VizBridge`.

### Resiliencia Operativa y Gestión de Keyframes

El despliegue prioriza exclusivamente el procesamiento de **I-frames (IDR)**. Esta decisión elimina la complejidad de reconstruir estados dependientes de P-frames, permitiendo que cada ciclo de inferencia sea una unidad atómica y autónoma. Para garantizar un tiempo de actividad (uptime) del 99.9% en la ingesta, el sistema encapsula el procesamiento de frames en un mecanismo de `**catch_unwind**`. Esto asegura que cualquier _panic_ en la lógica de inferencia o evaluación de estados sea contenido a nivel de frame, evitando que el error se propague y provoque la caída del gestor de sesiones RTSP.

## 2. Estructura del Workspace de Cargo y Dependencias

El sistema se implementa bajo un **Cargo Workspace (Rust Edición 2024)**, promoviendo una modularidad estricta que desacopla la orquestación binaria de la lógica de dominio reutilizable.

### Mapeo de Crates (Directorio `std/`)

- `**mana-types**`**:** Actúa como el proveedor de esquemas del sistema. Define los "wire formats" inmutables y versionados (`RawFrameV1`, `DetectionBatchV1`, `SceneMsgV1`) que garantizan la coherencia de datos en todo el pipeline.
- `**mana-geometry**`**:** Biblioteca de álgebra espacial para transformaciones de coordenadas (Crop Space a Frame Space) y manejo eficiente de máscaras RLE (`CompactMask`).
- `**mana-video**`**:** Abstracción crítica que proporciona el trait `FrameDecoder` y el componente `**BufferPool**`. El `BufferPool` es esencial para la estabilidad en producción, ya que recicla buffers de píxeles (ej. 6MB para 1080p RGB) evitando las penalizaciones de rendimiento por asignaciones frecuentes de memoria.
- `**mana-rtsp**`**:** Utilidades de bajo nivel para la manipulación de unidades NAL y negociación de flujos H.264.
- `**mana-viz**`**:** Traductor de tipos internos a primitivas del SDK de Rerun.io.

## 3. Jerarquía de Configuración: TOML y Variables de Entorno

La configuración de `mana-lite` sigue un modelo de capas para permitir despliegues reproducibles mediante infraestructura como código (IaC) pero localmente flexibles.

### Resolución de Configuración y Seguridad

El sistema carga inicialmente `config/mana.toml`, un archivo excluido del control de versiones vía `.gitignore` por seguridad. Posteriormente, se aplican overrides dinámicos mediante variables de entorno con prefijo `MANA_`. Este enfoque permite inyectar secretos y parámetros de red en tiempo de ejecución sin persistir datos sensibles en disco.

### Referencia de Variables Críticas en Despliegue

|   |   |   |
|---|---|---|
|Variable de Entorno|Campo `AppConfig`|Función Estratégica|
|`MANA_SOURCE_URL`|`cfg.source.url`|Endpoint RTSP de la cámara IP.|
|`MANA_SOURCE_PASSWORD`|`cfg.source.password`|Credencial sensible (sobrescribe TOML local).|
|`MANA_TRANSPORT`|`cfg.source.transport`|Selección de protocolo (TCP/UDP).|
|`MANA_BACKOFF_MAX_MS`|`cfg.ingest.reconnect_...`|Tiempo máximo de reintento en fallo de red.|
|`MANA_RERUN_ADDR`|`cfg.viz.rerun_addr`|Dirección del servidor de visualización Rerun.|
|`MANA_LOG_ROTATE`|`cfg.logger.rotate`|Política de rotación (`Hourly`, `Daily`, `Never`).|

## 4. Protocolo de Ingesta y Lógica de Autenticación RTSP

La adquisición de video se gestiona mediante el componente `RetinaReader`, diseñado para operar en redes IP volátiles.

### Gestión de Conexiones y Volatilidad

El sistema implementa una estrategia de reconexión basada en **Exponential Backoff con Jitter**. Tras una desconexión, el intervalo de espera se duplica hasta alcanzar el `MANA_BACKOFF_MAX_MS`, aplicando un factor de aleatoriedad (jitter) para evitar el fenómeno de "thundering herd" en despliegues masivos.

### Control de Calidad: Error Windowing

Para detectar degradaciones proactivamente, el sistema emplea **Error Windowing**. Se monitorea el **ratio de errores RTP** sobre una ventana deslizante de paquetes recibidos. Si el ratio supera el umbral de tolerancia, el sistema fuerza un reinicio de la sesión RTSP. Además, se aplica una deduplicación de keyframes: si un I-frame es idéntico byte a byte al anterior, se suprime el ciclo de inferencia para optimizar el uso de CPU en escenas estáticas.

## 5. Validación del Catálogo de Modelos y Blueprints

Los **Blueprints** funcionan como manifiestos de despliegue que definen el perfil operacional (ej. calibración vs. monitoreo clínico).

### Validación Pre-vuelo y Calibración

Antes de iniciar el pipeline, el módulo `validation` ejecuta un chequeo de integridad: asegura que los guardias de la FSM que referencian zonas existan efectivamente en el `ZoneCatalog`. Es un control de "stop-ship" que previene fallos lógicos en producción. Para reglas de profundidad, se aplica la fórmula de calibración monocular: \text{scene\_m} = \text{model\_val} \times \left( \frac{\text{reference\_scene\_m}}{\text{reference\_model\_m}} \right)

### Gating de Cascada: Resiliencia de Inferencia

El motor de cascada soporta dos modos de activación para modelos secundarios (ej. Face):

- **Same-Frame Gating (**`**same_frame = true**`**):** Ejecución inmediata sobre la detección del parent en el frame actual.
- **Track-Based Gating (**`**same_frame = false**`**):** El modelo hijo se dispara basándose en **tracks confirmados de frames previos**. Esto proporciona una resiliencia crítica contra "dropouts" momentáneos del detector primario, permitiendo mantener el análisis de rostro aunque el detector de persona falle en un fotograma específico.

## 6. Entorno de Producción: Observabilidad y Visualización

La observabilidad de grado de producción en `mana-lite` es multi-capa y de alto rendimiento.

### Estrategia de Logging y Serialización

El `LogManager` genera registros estructurados en formato **JSONL**. Para minimizar el impacto en la latencia, el sistema evita el uso de `serde_json` en favor de un **serializador manual optimizado** en `serialize.rs`, reduciendo drásticamente las asignaciones de memoria durante la exportación masiva de detecciones y máscaras RLE.

### Métricas de Salud y Estados Operativos

El `MetricsEngine` reporta estados de salud precisos:

- `**Healthy**`**:** Operación nominal.
- `**Blind**`**:** No se han recibido frames en el intervalo configurado (falla total de ingesta).
- `**Stale**`**:** El flujo de red es activo, pero el motor de inferencia presenta lag respecto a la tasa de llegada de frames.

### Visualización Espacial (Rerun.io)

El `VizBridge` mapea la telemetría interna a los siguientes **Entity Paths** estándar para facilitar el debugging en tiempo real:

- **Video:** `/world/camera/bgr`
- **Detecciones:** `/world/camera/detections/{class_id}/{index}` (usando `Boxes2D`).
- **Zonas:** `/world/camera/zones/{zone_name}`
- **Estado de Habitación:** `/pipeline/state/room`

Este protocolo asegura que cada despliegue de `mana-lite` sea observable, robusto y capaz de mantener la integridad operativa requerida en entornos clínicos de misión crítica.