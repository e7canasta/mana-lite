# Guía de Diseño de Lógica de Control: Estados Clínicos y Determinismo en Mana Lite

## 1. Fundamentos del Modelo de Ejecución: El Superloop PLC

En el diseño de sistemas de monitoreo clínico de alta fidelidad, la impredictibilidad es un vector de riesgo inaceptable. Mana Lite mitiga este riesgo mediante la adopción de una arquitectura de **Superloop PLC**, un modelo de ejecución síncrono y de un solo hilo que garantiza el determinismo temporal y la integridad de la lógica de seguridad. A diferencia de los sistemas asíncronos concurrentes, donde las condiciones de carrera pueden corromper la evaluación de estados, el Superloop asegura que cada ciclo de control se ejecute sobre un "snapshot" coherente del entorno.

### 1.1. Análisis de la Arquitectura de Ciclo Único y Memoria Determinista

La arquitectura de Mana Lite se consolida en un único binario estático que opera bajo un modelo síncrono bloqueante. Aunque utiliza un runtime de Tokio en modo `current-thread` para la gestión de I/O, el pipeline se comporta como una secuencia lineal donde solo se consulta un "future" por ciclo. Este enfoque elimina el "work-stealing" y la migración de tareas inherentes a los modelos asíncronos masivos, los cuales generan trazas de pila (stack traces) ininteligibles para la auditoría de fallos críticos.

Para optimizar el flujo de datos, se implementa el patrón de **Arena (CycleContext)**. Una estructura única posee todos los datos intermedios del ciclo; cada fase toma préstamos (borrows) de este contexto, llenando sus ranuras sin realizar nuevas asignaciones de memoria en la ruta caliente (hot path). Al final de cada ciclo, los vectores se limpian (`clear()`) pero no se liberan, garantizando un comportamiento de memoria predecible y una fragmentación nula.

### 1.2. Desglose de las Siete Fases del Ciclo

El motor de ejecución sigue un orden inmutable de siete fases. Cada fase debe completarse totalmente antes de ceder el control a la siguiente, asegurando que la información clínica fluya sin saltos lógicos.

|   |   |   |
|---|---|---|
|Fase|Responsabilidad Crítica|Motor Asociado|
|**TIMERS**|Actualización de contadores de sistema y latencia.|`InternalClock`|
|**EVALUATE**|Pre-evaluación del contexto y limpieza del Arena.|`CycleContext::clear`|
|**INGEST**|Captura y drenaje de frames H.264.|`RetinaReader`|
|**INFER**|Ejecución de modelos de visión (YOLO/Depth).|`CascadeScheduler`|
|**ZONES**|Intersección de bboxes con regiones espaciales.|`ZoneEngine`|
|**FSM**|Evaluación de la lógica de estados clínicos.|`FsmEngine`|
|**PUBLISH**|Emisión atómica de eventos JSONL y telemetría.|`LogManager` / `VizBridge`|

### 1.3. Garantía de Tiempo de Ejecución (WCET)

La principal ventaja de este modelo es la capacidad de medir el **Worst-Case Execution Time (WCET)**. Al evitar la concurrencia, el tiempo total del ciclo es simplemente la suma de sus fases. Si el Superloop excede el umbral de tiempo esperado, el sistema emite una alerta de salud inmediata. Esta rigidez es fundamental para la auditoría clínica: permite recrear exactamente qué dato de visión causó una transición de estado específica, vinculando cada decisión con un timestamp de pared (wall-clock) indiscutible.

## 2. Configuración de Zonas y Reglas de Histéresis Temporal

Las zonas espaciales transforman los datos de píxeles crudos en semántica clínica. Sin embargo, para alimentar una FSM de seguridad, estas zonas requieren un filtrado de persistencia que ignore el ruido de detección y las oclusiones momentáneas.

### 2.1. Mecanismo de Intersección AABB-AABB

Mana Lite emplea un algoritmo de intersección **AABB-AABB** (Axis-Aligned Bounding Box) bajo un criterio estrictamente conservador (ADR-014). A diferencia de los sistemas comerciales que exigen un umbral de IoU (Intersection over Union) o que el centro de masa esté contenido, Mana Lite marca una zona como ocupada si existe **cualquier solapamiento** entre el bbox del objeto y la zona. Esto maximiza la sensibilidad en escenarios críticos, como cuando un paciente apenas toca el borde de la cama.

### 2.2. Implementación de Timers Ton/Tof (Industria 4.0)

Para estabilizar la señal, se aplican lógicas de temporización industrial:

- **Timer Tof (Off-Delay / Histéresis)**: Utiliza `hysteresis_ms` para retrasar la emisión de `zone_vacated`. Si un track se pierde por una oclusión de 200ms (ej. una enfermera pasando), la zona permanece "ocupada" si el timer es superior a ese intervalo.
- **Timer Ton (On-Delay / Confirmación)**: Exige que la presencia sea persistente antes de disparar el estado de ocupación, mitigando falsos positivos esporádicos causados por artefactos de compresión en el stream.

### 2.3. Gestión de Identidad y SORT Tracking

La estabilidad espacial depende del **Tracker SORT** (Simple Online and Realtime Tracking). Este motor utiliza un filtro de **Kalman de 7 dimensiones** con un vector de estado `[cx, cy, s, r, dcx, dcy, ds]` para modelar la velocidad constante de los sujetos. La asociación de datos entre frames se resuelve mediante el **Algoritmo Húngaro** (Kuhn-Munkres) con una complejidad O(n^3), que garantiza el matching global óptimo entre nuevas detecciones y tracks existentes. Esta persistencia de identidad permite que la lógica de zonas emita eventos vinculados a un `track_id`, diferenciando entre múltiples ocupantes en una misma región.

## 3. Arquitectura de la Máquina de Estados Finitos (FSM) Clínica

La FSM es el núcleo intelectual del sistema. Al estar definida de forma declarativa en el archivo `fsm.toml`, permite que la política clínica sea modificada por ingenieros de dominio sin alterar el código fuente en Rust.

### 3.1. Jerarquía y Prioridad de Evaluación

El determinismo de la FSM se basa en la evaluación secuencial. En cada ciclo, se evalúan primero las **transiciones globales (wildcards)** y luego las transiciones del estado actual. El orden de aparición en el archivo TOML dicta la prioridad absoluta: la primera transición cuyos guardias se cumplan es la que se ejecuta. Esto evita la "inanición" de estados críticos y garantiza que el sistema siempre tenga un único estado válido.

### 3.2. Definición de Guardias (Guards) Multidimensionales

Los guardias son los centinelas booleanos que validan las transiciones. Mana Lite distingue entre estados de presencia y eventos de flanco:

- **Ocupación**: `zone_present` (booleano de estado), `zone_occupied` (disparo por flanco de entrada), `zone_vacated` (flanco de salida), y `all_zones_vacant`.
- **Geometría**: `depth_rule` (basado en estadísticas de percentiles del mapa de profundidad).
- **Integridad**: `data_stale` (vinculado al monitor de salud).

### 3.3. Lógica de Dwell Timers y Reseteo Instantáneo

Para eventos que requieren persistencia, como la detección de "caída" o "salida de cama", se utiliza `min_duration_ms` (Dwell Timer). Bajo los principios de seguridad PLC, si el guardia de una transición falla en **un solo frame**, el dwell timer se resetea inmediatamente a cero. No existe la acumulación parcial; el cumplimiento de la condición debe ser absoluto y continuo durante todo el intervalo configurado.

### 3.4. El Mecanismo de Latching (Enganche)

Para gestionar la pérdida de visibilidad, se implementan banderas de persistencia como `face_was_inside`. Este "latch" permite que la FSM distinga entre una salida voluntaria del paciente (donde la cara fue vista dentro de la zona de interés antes de desaparecer) y una pérdida de datos por fallo técnico, permitiendo rutas de recuperación diferenciadas.

## 4. Gestión de Crisis: Transiciones Wildcard y Salud del Sistema

La resiliencia clínica exige que el sistema reconozca su propia incapacidad de procesar datos. La FSM debe ser capaz de entrar en estados de seguridad de forma proactiva.

### 4.1. Transiciones Wildcard (`from = "*"`)

Las reglas configuradas con `from = "*"` tienen la prioridad más alta y se evalúan independientemente del estado actual. Su función principal es el manejo de fallos: ante la activación del guardia `data_stale`, el sistema salta inmediatamente a un estado de excepción (`blind` o `stale`), alertando que el monitoreo ya no es confiable.

### 4.2. Taxonomía de Estados de Salud

El sistema categoriza su operatividad según el flujo de datos:

- **Healthy**: Flujo de frames constante e inferencia activa.
- **Stale**: Se reciben frames (latido de red), pero no hay inferencia nueva (posible fallo del motor de IA).
- **Blind**: No se han recibido unidades NAL del ingest en el intervalo `data_stale_ms` (fallo de red o cámara).

### 4.3. Recuperación Determinista

Cuando el monitor de salud detecta el restablecimiento de la señal, emite un evento `Health::Recovered`. La FSM procesa este evento para transicionar desde el estado de error hacia un estado operativo inicial (ej. `searching`), garantizando que el sistema regrese a un estado conocido sin intervención humana una vez recuperada la integridad de los datos.

## 5. Optimización y Determinismo: I-Frame Gating y Modo Ghost

En dispositivos de borde (Edge), el rendimiento computacional es un requisito de seguridad para evitar el estrangulamiento térmico (thermal throttling) de la GPU/CPU.

### 5.1. Gating a Nivel de NAL (Network Abstraction Layer)

Mana Lite optimiza el consumo de recursos mediante la inspección de unidades NAL en el stream H.264 antes de la decodificación. Al identificar los tipos de unidades (ADR-004), el sistema descarta los frames P y B sin procesar un solo píxel, enfocándose únicamente en los **I-frames** (keyframes). Esta técnica reduce el uso de CPU en un **97%** en cámaras con un GOP alto, permitiendo que el dispositivo opere a temperaturas seguras.

### 5.2. El Concepto de Ghost Mode (Modo Fantasma)

El **Ghost Mode** es la consecuencia lógica y necesaria del I-frame gating. Aunque la inferencia solo ocurra cada 1 o 2 segundos (dependiendo del GOP), el Superloop PLC sigue girando a su frecuencia nominal. Durante los ciclos donde no hay una imagen nueva para inferir, el sistema mantiene la evaluación de la FSM y los timers utilizando las "detecciones fantasma" (las últimas conocidas). Esto asegura que los heartbeats de salud se emitan y que los contadores de tiempo de la FSM sigan avanzando con precisión milimétrica.

### 5.3. Latencia Clínica y Seguridad del Paciente

Dado que los eventos de movilidad humana en un entorno clínico (como levantarse de una cama) ocurren en escalas de segundos, una frecuencia de inferencia de **0.5 a 2 fps** es técnicamente suficiente. Este equilibrio permite un ahorro energético masivo sin comprometer la ventana de reacción de enfermería, siempre que el sistema mantenga su latido de control (PLC beat) constante a través del Ghost Mode.

**Validación de Inicio:** Todo despliegue debe ser validado mediante las herramientas de inspección de catálogo, asegurando que los identificadores de modelos y zonas en `fsm.toml` coincidan exactamente con las definiciones en `models.toml` y `zones.toml` antes de la ejecución en producción.