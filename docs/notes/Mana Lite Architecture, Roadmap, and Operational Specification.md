Estas fuentes detallan la arquitectura técnica y el manual operativo de **Mana Lite**, un sistema de visión artificial especializado en el monitoreo clínico mediante una estructura de **binario único** y procesamiento secuencial. El diseño se fundamenta en un **superloop tipo PLC** que gestiona fases de ingesta, inferencia en cascada, seguimiento de entidades y evaluación de una máquina de estados para detectar eventos en habitaciones hospitalarias. La documentación describe un flujo de datos configurado mediante archivos **TOML**, donde la percepción se refina a través de regiones de interés dinámicas y políticas de consolidación de modelos como detección de personas, posturas, rostros y mapas de profundidad. Para la observabilidad, el sistema integra tres canales: logs de texto para salud operativa, **JSONL** para análisis forense estructurado y una interfaz en **Rerun** para la visualización de métricas y telemetría en tiempo real. Finalmente, la hoja de ruta subraya la evolución hacia el seguimiento avanzado de pacientes y la calibración de reglas clínicas basadas en evidencia numérica y espacial.

La arquitectura **"PLC superloop"** es uno de los principios de diseño fundamentales de Mana Lite, caracterizándose por ser un sistema de **un solo proceso, un solo hilo y fases secuenciales**. A diferencia de sistemas complejos que utilizan múltiples procesos o canales de comunicación (IPC), esta arquitectura ejecuta todas sus tareas en un bucle infinito y determinista, similar al funcionamiento de un Controlador Lógico Programable (PLC).

### Estructura de las 7 Fases Secuenciales

El ciclo de vida de cada frame se procesa a través de **siete fases obligatorias** que se ejecutan en orden estricto dentro del bucle principal:

1. **TIMERS:** Se avanzan los contadores de permanencia (_dwell counters_) y se verifican los temporizadores de estado.
2. **EVALUATE:** Se evalúan los resguardos (_guards_) de la Máquina de Estados Finatarios (FSM) frente a las detecciones actuales.
3. **INGEST:** Se lee un paquete de red del flujo RTSP y se decodifica si se trata de un _keyframe_ (I-frame).
4. **INFER:** Se ejecutan los modelos de IA (detección, pose, segmentación, etc.) y se actualiza el seguimiento (_tracking_). Esta fase es el **cuello de botella** del sistema.
5. **ZONES:** Se mapean las detecciones a las zonas espaciales definidas y se actualiza el estado de ocupación.
6. **FSM:** Se evalúan las transiciones de estado basadas en las reglas de negocio y se avanza el estado del sistema.
7. **PUBLISH:** Se emiten y limpian los eventos acumulados hacia la salida estándar (JSONL).

### Características Operativas Clave

- **Sin Concurrencia Interna:** El diseño evita explícitamente el uso de canales (_channels_), hilos adicionales (_spawn_) o comunicación entre procesos (IPC) para el flujo principal de datos. Toda la información se maneja mediante **referencias en memoria**, lo que reduce drásticamente la latencia.
- **Velocidad del Ciclo:** El bucle corre tan rápido como la fase más lenta lo permita, que generalmente es la de inferencia (INFER). Por ejemplo, mientras la fase de temporizadores tarda menos de 1µs, la inferencia puede tomar entre 20ms y 200ms por modelo.
- **Manejo de Errores:** Para evitar que un fallo en un frame detenga el sistema, el procesamiento se envuelve en mecanismos que capturan pánicos, permitiendo que el bucle continúe con el siguiente ciclo si una fase específica falla.
- **Simplicidad frente a Mana OS:** Mientras que el sistema "Full Mana OS" utiliza más de 8 procesos coordinados por IPC, Mana Lite consolida todo en este **único binario y hilo**, lo que facilita su despliegue y monitoreo en entornos de recursos limitados.

---


La arquitectura **"PLC superloop"** es uno de los principios de diseño fundamentales de Mana Lite, caracterizándose por ser un sistema de **un solo proceso, un solo hilo y fases secuenciales**. A diferencia de sistemas complejos que utilizan múltiples procesos o canales de comunicación (IPC), esta arquitectura ejecuta todas sus tareas en un bucle infinito y determinista, similar al funcionamiento de un Controlador Lógico Programable (PLC).

### Estructura de las 7 Fases Secuenciales

El ciclo de vida de cada frame se procesa a través de **siete fases obligatorias** que se ejecutan en orden estricto dentro del bucle principal:

1. **TIMERS:** Se avanzan los contadores de permanencia (_dwell counters_) y se verifican los temporizadores de estado.
2. **EVALUATE:** Se evalúan los resguardos (_guards_) de la Máquina de Estados Finatarios (FSM) frente a las detecciones actuales.
3. **INGEST:** Se lee un paquete de red del flujo RTSP y se decodifica si se trata de un _keyframe_ (I-frame).
4. **INFER:** Se ejecutan los modelos de IA (detección, pose, segmentación, etc.) y se actualiza el seguimiento (_tracking_). Esta fase es el **cuello de botella** del sistema.
5. **ZONES:** Se mapean las detecciones a las zonas espaciales definidas y se actualiza el estado de ocupación.
6. **FSM:** Se evalúan las transiciones de estado basadas en las reglas de negocio y se avanza el estado del sistema.
7. **PUBLISH:** Se emiten y limpian los eventos acumulados hacia la salida estándar (JSONL).

### Características Operativas Clave

- **Sin Concurrencia Interna:** El diseño evita explícitamente el uso de canales (_channels_), hilos adicionales (_spawn_) o comunicación entre procesos (IPC) para el flujo principal de datos. Toda la información se maneja mediante **referencias en memoria**, lo que reduce drásticamente la latencia.
- **Velocidad del Ciclo:** El bucle corre tan rápido como la fase más lenta lo permita, que generalmente es la de inferencia (INFER). Por ejemplo, mientras la fase de temporizadores tarda menos de 1µs, la inferencia puede tomar entre 20ms y 200ms por modelo.
- **Manejo de Errores:** Para evitar que un fallo en un frame detenga el sistema, el procesamiento se envuelve en mecanismos que capturan pánicos, permitiendo que el bucle continúe con el siguiente ciclo si una fase específica falla.
- **Simplicidad frente a Mana OS:** Mientras que el sistema "Full Mana OS" utiliza más de 8 procesos coordinados por IPC, Mana Lite consolida todo en este **único binario y hilo**, lo que facilita su despliegue y monitoreo en entornos de recursos limitados.

---

El manejo de la latencia en la arquitectura **PLC superloop** de Mana Lite se basa en un diseño determinista que prioriza la **frescura de los datos** y la **eliminación de sobrecargas** operativas. Al ser un sistema de un solo hilo y proceso, evita la latencia inherente a la comunicación entre procesos (IPC) y la sincronización de hilos.

A continuación, se detallan las estrategias específicas para gestionar la latencia:

### 1. Política de "El último frame gana" (_Latest-frame-wins_)

Para mantener el procesamiento en tiempo real, el `IngestEngine` drena todos los paquetes acumulados en el buffer del lector RTSP, pero **solo entrega para procesamiento el frame clave (IDR) más reciente**. Si la fase de inferencia tarda más que el intervalo entre frames de la fuente, el sistema descarta los frames antiguos (`keyframes_dropped`) para asegurar que el pipeline siempre trabaje con la imagen más actual.

### 2. Filtrado y Decodificación Optimizada

- **I-frame Gating:** El sistema procesa exclusivamente **I-frames** (keyframes), ignorando los P-frames y B-frames. Esto reduce drásticamente la carga de decodificación y elimina la necesidad de esperar a que se reconstruya una secuencia completa de video.
- **Bandera LOW_DELAY:** El decodificador de software está configurado con la bandera `LOW_DELAY`, que elimina el buffer interno de un frame que FFmpeg suele mantener para el reordenamiento de B-frames, entregando el primer IDR de forma inmediata.
- **Single Threaded Decoder:** Se desactiva el multihilo en el decodificador para evitar el costo de los cambios de contexto y asegurar una latencia predecible.

### 3. Eficiencia en el Procesamiento de Inferencia

La fase de **INFER** es el principal cuello de botella (20-200ms por modelo). Para mitigar esto, se utilizan:

- **Cascada de Modelos:** Los modelos secundarios (como detección de rostros o pose) solo se ejecutan si el modelo primario detecta un objetivo válido (ej. una persona). Si no hay detección, estos modelos se omiten (_skip_), ahorrando tiempo de computo.
- **Regiones de Interés (ROI):** El uso de **crops** (estáticos o dinámicos) reduce la cantidad de píxeles que el modelo debe procesar. Un recorte de la mitad del área puede reducir hasta 4 veces la cantidad de píxeles procesados, acelerando la inferencia.

### 4. Gestión de Memoria y Datos

- **BufferPool:** Para evitar penalizaciones de rendimiento por asignaciones frecuentes de memoria (un frame 1080p RGB pesa ~6MB), el sistema utiliza un pool de buffers que recicla los espacios de memoria ya asignados.
- **Referencias en Memoria:** Al consolidar todo en un solo binario, el paso de datos entre fases se realiza mediante referencias en memoria, eliminando el tiempo de serialización y transporte requerido en sistemas distribuidos.
- **Despacho Estático:** El uso de genéricos en Rust permite un despacho estático sin tablas de funciones virtuales (vtable), optimizando la velocidad de ejecución a nivel de CPU.

### 5. Monitoreo de la "Salud" de Latencia

El sistema utiliza métricas como el `gap_ms` (tiempo entre frames procesados) y el `freshness` (relación entre el periodo esperado y el retraso real) para detectar degradaciones. Si el flujo se vuelve demasiado lento, el monitor de salud marca el estado como `Stale` (frames llegando pero procesados lentamente) o `Blind` (sin frames), permitiendo la ejecución de lógicas de recuperación.


---

En el contexto del sistema `mana-lite`, el **BufferPool** es una utilidad de gestión de memoria situada en la crate `mana-media` (antes `mana-video`, fusionada en el Sprint 4) diseñada para optimizar el rendimiento del pipeline de video.

Su función principal y beneficios se detallan a continuación:

### 1. Prevención de asignaciones frecuentes (Allocations)

La razón fundamental de su existencia es evitar la penalización de rendimiento que suponen las **asignaciones frecuentes de memoria de gran tamaño**. En un sistema de procesamiento de video en tiempo real, cada frame es pesado; por ejemplo, un solo frame RGB a resolución 1080p ocupa aproximadamente **6 MB**. Sin un pool, el sistema tendría que solicitar y liberar esos 6 MB constantemente para cada frame, lo que generaría una carga innecesaria en el asignador de memoria y fragmentación.

### 2. Ciclo de Vida y Reciclaje

El `BufferPool` funciona manteniendo una estructura `Mutex<Vec<Vec<u8>>>` que almacena buffers ya asignados para ser **reutilizados** en lugar de eliminados. El proceso sigue tres etapas:

1. **Adquisición (Acquire):** El hilo de ingesta solicita un buffer disponible al pool para un nuevo frame.
2. **Llenado (Fill):** El decodificador de video escribe los datos de los píxeles directamente en ese buffer.
3. **Liberación (Release):** Una vez que el procesamiento del frame (inferencia y visualización) ha terminado, el buffer se devuelve al pool mediante una función de liberación para que el siguiente frame lo use.

### 3. Control de Memoria y Límites

Para garantizar la estabilidad del sistema, el pool está **acotado (bounded)**. Esto significa que si el número de buffers liberados excede un límite configurado (`max_bufs`), los buffers sobrantes se descartan. Esta medida previene **fugas de memoria** en caso de errores en las rutas de ejecución del pipeline.

### 4. Impacto en la Latencia

Al integrarse en la arquitectura de **"PLC superloop"** de un solo hilo, el uso del `BufferPool` permite que el paso de datos entre las distintas fases (como `INGEST` e `INFER`) se realice mediante **referencias en memoria**, eliminando la necesidad de copiar datos o serializarlos, lo que reduce drásticamente la latencia general del sistema.


---

La política **"Latest-frame-wins"** (el último frame gana) es la estrategia central de gestión de latencia del sistema para asegurar un procesamiento en **tiempo real**, priorizando siempre la **frescura de los datos** sobre la continuidad secuencial de todos los frames.

Funciona mediante los siguientes mecanismos técnicos:

### 1. Drenado del Buffer de Ingesta

El `IngestEngine` actúa como un filtro entre la red y el pipeline de procesamiento. En cada ciclo del "PLC superloop", el motor drena (vacía) todos los paquetes acumulados en el buffer del lector RTSP (`RetinaReader`). En lugar de procesar cada frame que llega en orden cronológico, el sistema **descarta todos los frames anteriores** y solo entrega para decodificación e inferencia el **keyframe (IDR) más reciente**.

### 2. Filtrado de Frames (I-frame Gating)

Para maximizar la eficiencia y reducir la carga de la CPU, el sistema aplica un filtrado estricto:

- **Solo Keyframes:** Se ignoran sistemáticamente los P-frames y B-frames, ya que los I-frames son los únicos que contienen la información completa de la imagen sin depender de frames anteriores.
- **Deduplicación:** Si el nuevo keyframe detectado es byte por byte idéntico al anterior, se suprime para evitar ciclos de inferencia redundantes.

### 3. Sincronización con la Velocidad de Inferencia

Dado que la fase de **INFER** (inferencia de IA) es el principal cuello de botella del sistema (tardando entre 20ms y 200ms por modelo), el pipeline a menudo corre más lento que la fuente de video.

- Si la inferencia de un frame tarda más de lo que tarda en llegar el siguiente, el sistema simplemente **reemplaza el frame viejo** que estaba esperando por el nuevo que acaba de llegar.
- Esto evita que se acumule un "backlog" o retraso acumulativo, manteniendo la latencia constante incluso bajo carga pesada.

### 4. Observabilidad y Métricas

El comportamiento de esta política se monitorea constantemente a través de métricas específicas que se pueden consultar en los logs JSONL y en Rerun:

- **`keyframes_seen`**: Total de keyframes que llegaron desde la cámara.
- **`keyframes_dropped`**: Cantidad de keyframes que fueron descartados por ser "viejos" antes de llegar a ser procesados.
- **`drop_ratio`**: La fracción de trabajo descartado frente al visto, lo que permite diagnosticar si el hardware es insuficiente para el modelo seleccionado.

En la herramienta de visualización **Rerun**, este fenómeno se observa claramente en el panel de **Stream**, donde la métrica `dropped` confirma que el sistema está saltando frames para mantenerse al día con el flujo en vivo.

---

El **I-frame gating** (o filtrado de frames clave) es un mecanismo de optimización y gestión de latencia dentro del motor de ingesta de Mana Lite. Su función principal es **reducir drásticamente la carga de la CPU** y asegurar que el sistema procese únicamente la información visual más relevante y "fresca" proveniente del flujo RTSP.

A continuación se detalla su funcionamiento y propósito según los fuentes:

### Funcionamiento Técnico

- **Selección de Frames IDR:** El sistema escanea las unidades NAL (Network Abstraction Layer) del flujo H.264 buscando el **Tipo 5**, que corresponde a los frames **IDR** (Instantaneous Decoder Refresh) o keyframes.
- **Descarte de P-frames:** Mientras el flujo de video puede contener frames de predicción (P-frames) o bidireccionales (B-frames), el `IngestEngine` los cuenta y los **descarta sistemáticamente**.
- **Política de "El último gana":** El motor de ingesta vacía (_drains_) todos los paquetes acumulados en el buffer del lector, pero **solo entrega para decodificación el keyframe más reciente**. Esto evita que el sistema intente procesar una cola de frames antiguos si la fase de inferencia se ha retrasado.
- **Deduplicación:** Si el sistema detecta que un nuevo I-frame es byte por byte idéntico al anterior, lo suprime para evitar ciclos de inferencia redundantes.

### Objetivos del I-frame Gating

1. **Minimizar el Overhead de Decodificación:** Al ignorar los frames intermedios, el decodificador no tiene que reconstruir imágenes basadas en deltas de frames anteriores, lo que ahorra recursos significativos de computación.
2. **Imágenes Auto-contenidas:** Los I-frames contienen toda la información necesaria para representar la imagen completa, lo que garantiza que cada ciclo de la "cascada de inferencia" trabaje con una base sólida y completa.
3. **Baja Latencia Real:** En conjunto con la configuración `LOW_DELAY` del decodificador (que elimina el buffer interno de un frame de FFmpeg), el gating asegura que el primer IDR detectado se procese de inmediato sin esperas.

### Configuración

Este comportamiento es una de las decisiones de diseño fundamentales (ADR-007) y se puede controlar mediante el campo `keyframes_only` en el archivo de configuración `mana.toml` o a través de la variable de entorno **`MANA_KEYFRAMES_ONLY`**.

---

Si la decodificación de un frame falla en Mana Lite, el sistema está diseñado para ser **resiliente y determinista**, asegurando que un error en un cuadro individual no detenga la operación general del pipeline.

Esto es lo que ocurre técnicamente cuando falla el proceso de decodificación:

### 1. Resiliencia del Pipeline (Sin Caídas)

El ciclo principal del sistema, el **PLC superloop**, envuelve la lógica de procesamiento de cada frame en un mecanismo de **`catch_unwind`**. Esto garantiza que si ocurre un fallo crítico o un "pánico" durante la decodificación o el procesamiento, el error sea capturado en el límite de la fase, incrementando los contadores de error pero permitiendo que el binario continúe su ejecución con el siguiente ciclo.

### 2. Captura Forense para Debugging

Una característica clave del subsistema de snapshots es que, cuando una decodificación falla, el sistema puede **guardar la unidad NAL H.264 original** (el paquete de red crudo) en el disco. Esto es fundamental para el diagnóstico de problemas de red o corrupción de stream, permitiendo a los desarrolladores analizar el paquete exacto que el decodificador de software no pudo procesar.

### 3. Transición de Salud a estado "Blind"

El motor de métricas monitorea constantemente la "frescura" de los datos. Si las fallas de decodificación son persistentes y no se procesan frames exitosamente durante un periodo definido (`data_stale_ms`), el monitor de salud transiciona el sistema a un estado **"Blind"** (Ciego).

- Esta transición activa los **Wildcard Transitions** en la Máquina de Estados (FSM), permitiendo que el sistema mueva automáticamente su lógica de negocio a un estado de error o alerta global.

### 4. Impacto en las Métricas y Logs

- **Métricas de Ingesta:** La falla afectará los indicadores de rendimiento. El valor de **`processed_hz`** (frames procesados por segundo) disminuirá, mientras que el **`gap_ms`** (el tiempo transcurrido entre frames procesados con éxito) aumentará en los gráficos de Rerun y en los logs.
- **Eventos JSONL:** En el archivo de registro forense, el evento de tipo `frame` reflejará la anomalía o simplemente habrá una ausencia de eventos de inferencia asociados a ese ID de frame.

### 5. Omisión de Fases Posteriores

Dado que el pipeline es secuencial, si la fase de **INGEST** (donde ocurre la decodificación) no produce un buffer de píxeles válido, las fases subsiguientes de **INFER** (inferencia), **ZONES** y **FSM** no tendrán datos sobre los cuales operar en ese ciclo específico, saltando efectivamente el procesamiento de esa imagen.



¿Cómo se gestiona la latencia en el superloop?

La gestión de la latencia en la arquitectura **PLC superloop** de Mana Lite se fundamenta en un diseño determinista de un solo hilo que prioriza la **frescura de los datos** sobre el procesamiento secuencial de cada frame de video. Al eliminar la concurrencia compleja (sin canales, hilos adicionales ni IPC interno), el sistema reduce drásticamente los tiempos de sincronización y context-switching.

Las estrategias clave para gestionar la latencia son:

### 1. Política "Latest-frame-wins" (El último frame gana)

Para asegurar que el sistema opere en tiempo real, el motor de ingesta emplea una estrategia de vaciado agresivo. En cada ciclo, el `IngestEngine` drena todos los paquetes acumulados en el buffer del lector RTSP, pero **solo entrega para procesamiento el frame clave (IDR) más reciente**. Si la fase de inferencia tarda más que el intervalo entre frames, los keyframes "viejos" se descartan automáticamente para evitar acumulaciones de retraso (_backlog_).

### 2. I-frame Gating y Optimización del Decodificador

El sistema minimiza el costo computacional de la decodificación mediante dos mecanismos:

- **I-frame Gating:** Se descartan sistemáticamente todos los P-frames y B-frames, procesando únicamente frames independientes (IDR).
- **Configuración Low Delay:** El decodificador FFmpeg se configura con la bandera `LOW_DELAY`, que elimina el buffer interno de un frame que usualmente se mantiene para reordenar B-frames, entregando la imagen de forma inmediata.
- **Decodificación de un solo hilo:** Se desactiva el multihilo en el decodificador para garantizar una latencia predecible y evitar cambios de contexto costosos.

### 3. Gestión de Memoria con BufferPool

Para evitar la penalización de rendimiento de las asignaciones frecuentes de memoria (un frame 1080p RGB pesa ~6MB), el sistema utiliza un **BufferPool**. Este componente mantiene un pool acotado de buffers pre-asignados que se reciclan en cada ciclo: se adquieren para el decodificador y se liberan una vez terminada la inferencia y visualización.

### 4. Cascada de Inferencia y Regiones de Interés (ROI)

Dado que la fase de **INFER** es el cuello de botella (tardando entre 20ms y 200ms por modelo), se aplican optimizaciones espaciales:

- **Crops (Recortes):** El uso de ROIs estáticos o dinámicos reduce la cantidad de píxeles procesados. Un recorte a la mitad del área puede reducir hasta 4 veces la carga de trabajo del modelo.
- **Cascadas:** Los modelos secundarios (como detección de caras o pose) solo se ejecutan si el modelo primario detecta un objetivo válido, evitando ciclos de computo innecesarios.

### 5. Fases Secuenciales Deterministas

El ciclo de vida de un frame se procesa en siete fases con objetivos de tiempo estrictos (WCET):

1. **TIMERS:** (< 1µs) Avance de contadores.
2. **EVALUATE:** (< 10µs) Evaluación de resguardos de la FSM.
3. **INGEST:** (< 5ms) Lectura y decodificación.
4. **INFER:** (20-200ms) **Cuello de botella** del sistema.
5. **ZONES:** (< 10µs) Mapeo a zonas espaciales.
6. **FSM:** (< 10µs) Transiciones de estado.
7. **PUBLISH:** (< 100µs) Emisión de eventos.

El bucle corre tan rápido como la fase más lenta lo permite, y el monitoreo de salud detecta estados **"Stale"** (lento) o **"Blind"** (sin señal) para activar lógicas de recuperación si la latencia degrada la operación.