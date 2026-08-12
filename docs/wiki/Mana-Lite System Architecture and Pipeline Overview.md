> ⚠️ **Página generada, desactualizada.** Se generó contra el commit
> `ad24740d`. Divergencias conocidas al 2026-08-12, específicas de esta
> página:
>
> - **El super loop ya no existe.** Se describe una sola task con `tokio::select!` entre ingesta y reloj; desde el 2026-08-11/12 son tres etapas con dueños de ejecución distintos, unidas por slots que no bloquean (ADR-033, ADR-034). Los tipos `PipelineObserver`, `FanoutObserver` y `NullObserver` fueron borrados.
>
> - **El tracker cuenta en mediciones, no en scans.** `misses` y `hit_streak` se incrementaban una vez por tick del lazo; como el lazo tica más rápido que la evidencia, eso inventaba fallos de detección y ningún track llegaba a confirmarse. Desde el 2026-08-12 la asociación corre sólo cuando hay medición nueva y los scans sin medición usan `Tracker::age_at`, que envejece la vida del track sin contarla como fallo. `max_age_ms` y `tentative_max_age_ms` siguen en tiempo de pared.
>
> - **La compuerta de la cascada se borró del código.** Había una condición hardcodeada en `src/app/inference.rs` (`presence_track_count != 1`) que decidía si corría un modelo hijo. Duplicaba `requires_exact_count = 1` del blueprint contra otra fuente de datos y sin estar declarada en ningún catálogo. Desde el 2026-08-12 la condición vive **sólo** en las reglas del blueprint y la resuelve `cascade.rs`.
>
> No se corrige a mano: es un archivo **generado** y una corrección manual se
> pierde en la próxima regeneración, además de crear un segundo relato que
> compite con el primero. Lo que corresponde es regenerar contra `HEAD`.
>
> Fuentes autorizadas mientras tanto: [ARCHITECTURE.md](../../ARCHITECTURE.md)
> sobre ejecución, [HANDOFF.md](../../HANDOFF.md) y [`docs/adrs/`](../adrs/)
> sobre estado y decisiones, y [`workshop/MANUAL.md`](../../workshop/MANUAL.md)
> sobre cómo se opera y se lee la salida.

**Mana-lite** es una arquitectura de software avanzada diseñada para el **procesamiento de video en tiempo real** mediante visión artificial y lógica de seguimiento. El sistema opera a través de dos áreas principales: la **percepción**, que gestiona la decodificación y ejecución de modelos neuronales, y el **control**, que estabiliza los datos para determinar estados semánticos precisos. Su infraestructura utiliza un **vocabulario de dominio específico** para garantizar la seguridad de tipos al identificar modelos, zonas espaciales y comportamientos automáticos. La flexibilidad de la plataforma se basa en **blueprints**, configuraciones modulares que definen cómo se encadenan los detectores y cómo se interpretan los eventos capturados. Finalmente, el proyecto integra herramientas de **observabilidad y telemetría** para facilitar el monitoreo del flujo de datos y la depuración del sistema en vivo.

## ¿Cómo se dividen las responsabilidades entre Perception y Control?

La arquitectura de **mana-lite** se divide en dos dominios primarios: **Perception** (Percepción) y **Control**, diseñados para equilibrar el procesamiento intensivo de datos con una lógica de estado determinista.

A continuación se detalla cómo se distribuyen las responsabilidades:

### 1. Dominio de Perception (Percepción)

Este dominio se encarga del **"trabajo pesado"** relacionado con el procesamiento de señales de video y la ejecución de redes neuronales. Su objetivo principal es transformar paquetes de red RTSP en observaciones semánticas de alto nivel (por ejemplo: "hay una persona en estas coordenadas").

- **Ingestión y Decodificación:** Gestiona la conectividad RTSP, la reensamblaje de paquetes RTP y la decodificación de video H.264 (usualmente solo _keyframes_ IDR) en búferes de píxeles RGB/BGR.
- **Ejecución de Modelos (InferEngine):** Coordina la ejecución de la **cascada de modelos** YOLO. Esto incluye:
    - **Modelos Raíz:** Detecciones generales sobre el cuadro completo.
    - **Modelos Hijos:** Inferencia especializada (rostros, poses) sobre recortes dinámicos (_Dynamic Cropping_) basados en las detecciones del padre.
- **Consolidación de Detecciones:** Fusiona los resultados de múltiples modelos para evitar duplicados y asocia subcomponentes (como una cara) a una entidad padre (una persona) mediante cálculos de IoU (_Intersection over Union_).
- **Cadencia Variable:** Este dominio opera de forma reactiva; se activa cada vez que llega un nuevo cuadro de video procesable.

### 2. Dominio de Control (mana-control)

Este dominio actúa como el **"núcleo de toma de decisiones"**. Consume las observaciones "congeladas" generadas por Perception para gestionar la estabilidad temporal y la lógica de negocio.

- **Seguimiento de Entidades (Tracker):** Convierte detecciones discretas cuadro a cuadro en identidades estables (_Tracks_) utilizando **filtros de Kalman** para manejar oclusiones y ruido del detector.
- **Lógica de Ocupación:** Evalúa los rastros para determinar la cardinalidad de una habitación (Vacía, Única, Múltiple) basándose en políticas clínicas y tiempos de confirmación.
- **Motor de Zonas (ZoneEngine):** Evalúa la posición de las entidades respecto a regiones geométricas predefinidas (AABB) para emitir eventos de "Zona Ocupada" o "Vacía" con histéresis.
- **Máquina de Estados Finita (FSM):** Ejecuta la lógica conductual de alto nivel (estados como `in_bed` o `exiting`) evaluando "guardias" que dependen de las zonas, señales de la escena o reglas de profundidad.
- **Cadencia Fija (Scan Loop):** A diferencia de la percepción, el sistema de control pulsa a un **intervalo constante** (típicamente 200ms) para garantizar un comportamiento determinista en temporizadores y transiciones.

### El puente entre dominios

La comunicación entre ambos se realiza a través de una estructura llamada **ClinicalSample** (o `ProcessImage`), que empaqueta las observaciones consolidadas, los metadatos de salud del ciclo de inferencia y los datos de sensores adicionales para que el sistema de control los procese en su siguiente "tic".


## ¿Cómo se sincronizan Perception y Control si operan a cadencias distintas?


La sincronización entre los dominios de **Perception** y **Control** en mana-lite se logra mediante un desacoplamiento estratégico que utiliza una estructura de datos "congelada" y un sistema de cronometraje determinista. Mientras que la percepción es reactiva y variable, el sistema de control opera con un "latido" constante.

Aquí se detalla el mecanismo de sincronización:

### 1. El puente de datos: `ProcessImage` (o `ClinicalSample`)

Perception procesa cuadros de video solo cuando llegan _keyframes_ IDR, lo que genera una cadencia variable dependiendo del flujo RTSP. Al finalizar un ciclo de inferencia, todos los resultados se empaquetan en una estructura llamada **`ProcessImage`** (también referida como `ClinicalSample` en la capa de aplicación).

- Esta estructura actúa como un **puerto de entrada "congelado"** para el sistema de control, conteniendo las observaciones consolidadas, metadatos de salud y datos de sensores (como profundidad) de ese instante específico.

### 2. Abstracción temporal: `ScanTimeline` y `ControlStamp`

Para evitar inconsistencias causadas por latencias de red o de procesamiento, el sistema no depende directamente del reloj del sistema (_wall clock_), sino de una línea de tiempo virtual:

- **`ScanTimeline`**: Proporciona una base de tiempo monótona y controlada exclusivamente para el lazo de control, lo que permite un comportamiento determinista incluso durante reproducciones de datos grabados.
- **`ControlStamp`**: Cada decisión tomada en el ciclo de control se marca con este "sello", que vincula la decisión clínica con el ID del cuadro de video original y registra la **edad de las observaciones** (`observations_age_ms`). Esto permite que el sistema de control sepa exactamente qué tan antiguos son los datos que está evaluando.

### 3. Cadencias diferenciadas

- **Perception (Reactiva)**: Se activa por eventos (la llegada de un cuadro) y corre tan rápido como el hardware lo permita o según la tasa de entrada de video.
- **Control (Fija/Síncrona)**: Mana-control pulsa a una frecuencia constante, típicamente **5Hz (cada 200ms)**. Este ritmo fijo es vital para que los **temporizadores de persistencia** (_dwell timers_) y los **filtros de Kalman** funcionen correctamente, garantizando que una transición de estado (ej. "el paciente salió de la cama") sea estable y no dependa de si el video llega a 10 o 60 FPS.

### 4. Gestión de la obsolescencia (_Staleness_)

Debido a que operan a ritmos distintos, el sistema de control debe validar la frescura de los datos en cada "tic":

- En la fase inicial de cada ciclo de `scan()`, se verifica la edad del `ProcessImage`.
- Si los datos superan un umbral de tiempo configurado (`data_stale_ms`), el sistema marca la entrada como inválida y el motor de salud activa guardias de **"DataStale"** en la Máquina de Estados (FSM) para mover el sistema a un estado seguro o "ciego".

En resumen, la percepción entrega "fotos" semánticas de la escena cuando puede, y el control las consume a un ritmo constante, utilizando el **`ControlStamp`** para mantener la coherencia temporal entre lo que el modelo vio y lo que la lógica decide.


## ¿Cómo se gestiona el estado 'Stale' en la FSM?

