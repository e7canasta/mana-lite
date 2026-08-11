# El Viaje de un Frame: De Píxeles a Decisiones en mana-lite

Bienvenido a esta crónica técnica. Aquí descubriremos cómo **mana-lite**, un sistema de visión computacional de alto rendimiento, transforma una señal de video bruta en inteligencia operativa. Este sistema actúa como un puente entre el caos de los píxeles y la claridad de las decisiones clínicas, haciendo que lo complejo se vuelva invisible.

### 1. El Mapa del Sistema: Los Dos Dominios

Para entender **mana-lite**, debemos visualizar su estructura como un cerebro dividido en dos hemisferios: el dominio de **Perception** (Percepción) y el de **Control** (Control). Esta separación asegura que el sistema sea potente pero, sobre todo, predecible.

|   |   |   |
|---|---|---|
|Característica|Dominio de Percepción (Perception)|Dominio de Control (Control)|
|**Rol Principal**|Ingesta, decodificación y ejecución de IA (IA).|Estabilización temporal y reglas de negocio.|
|**Frecuencia**|Variable (según la llegada de video e IA).|Fija y determinista (cada 200ms).|
|**Objetivo Final**|Detección bruta (¿Qué hay ahí?).|Estado semántico (¿Qué está pasando?).|

Esta estructura permite que, incluso si el video sufre retrasos, el "latido" del sistema de control siga latiendo con precisión quirúrgica, preparándonos para la puerta de enlace: la red.

### 2. La Puerta de Entrada: Ingesta y Selección Crítica

El viaje comienza con paquetes de red RTSP. No todos los datos son iguales; procesar cada milisegundo de video agotaría los recursos sin beneficio real. Aquí aplicamos una **Política de Keyframes** estricta.

Descartamos los **P-frames** (que son solo listas de cambios parciales entre imágenes) y buscamos el **frame IDR (Keyframe)** más fresco. ¿Por qué? Porque un frame IDR es una imagen completa e independiente; es el "mapa entero" que nos permite empezar de cero con total fidelidad.

Para filtrar esta avalancha de datos, usamos tres componentes:

- **RetinaReader:** El centinela de la red. Maneja la conexión RTSP y asegura que los paquetes se reensamblen correctamente.
- **IngestEngine:** El selector de élite. Aplica la política de keyframes y realiza una **Deduplicación (Hashing)** de 64 bits. Si el código del cuadro es idéntico al anterior, el sistema sabe que la imagen es estática y la descarta, evitando gastar energía en procesar exactamente lo mismo.
- **BufferPool:** Un almacén de memoria inteligente. En lugar de crear y destruir espacio para imágenes constantemente, reutiliza "cunas" de memoria preexistentes para ganar velocidad.

Una vez seleccionado el cuadro perfecto, es hora de revelar la imagen oculta en el código H.264.

### 3. Revelando la Imagen: Decodificación y Preparación

El **FrameDecoder** es el artista que transforma unidades NAL (datos comprimidos) en un búfer de píxeles RGB/BGR utilizando `ffmpeg-next`.

En este sistema, el tiempo es el recurso más valioso. Por ello, configuramos el decodificador en modo **Low Delay**. Al desactivar el reordenamiento de cuadros (habitual en el cine para mayor suavidad), garantizamos que la imagen que vemos sea el presente absoluto. El **BufferPool** entra de nuevo en juego aquí, evitando el coste de crear nuevas imágenes y permitiendo que los píxeles fluyan directamente hacia el "pensamiento" de la inteligencia artificial.

### 4. La Cascada de Inteligencia: Inferencia YOLO

El **InferEngine** no lanza todos sus modelos a la vez. Organiza una "cascada" jerárquica para optimizar el esfuerzo computacional.

Primero, corre un **Modelo Raíz** (como un detector de personas en toda la imagen). Solo si encuentra algo, activa los **Modelos Hijos** (como detectores de rostros o poses). Lo más brillante ocurre después de la detección: la **Traducción de Coordenadas**. Dado que los modelos hijos ven "recortes" de la imagen, el sistema debe coser esas coordenadas de vuelta al marco original para que todo encaje en el mapa global.

Para estos recortes, el sistema emplea varias estrategias:

|   |   |   |
|---|---|---|
|Estrategia|Descripción|Caso de Uso|
|**Static ROI**|Región fija manual.|Vigilar una zona inamovible (ej. una cama).|
|**Dynamic Crop**|Recorte móvil en tiempo real.|Seguir a una persona mientras camina.|
|**Upper Square**|Cuadrado en la mitad superior.|Capturar rostros con precisión incluso en movimiento.|

Esta cascada asegura que no busquemos rostros en el aire o en las paredes, sino solo donde ya sabemos que hay una persona.

### 5. El Refinado de Datos: Consolidación de Detecciones

Cuando múltiples modelos ven "lo mismo", el **DetectionConsolidator** actúa como un editor jefe. Utiliza la técnica **IoU (Intersection-over-Union)** para medir el solapamiento de las detecciones; si dos cuadros se enciman casi totalmente, los fusiona en una sola entidad.

Un proceso crítico es el **Acoplamiento de Rostro a Persona**. Para que un rostro se "pegue" a un cuerpo, usamos umbrales configurables que validan tres requisitos:

1. **Cobertura:** El rostro debe estar contenido dentro del cuerpo.
2. **Posición Vertical:** El rostro debe estar en la parte superior (según el ratio `face_max_center_y_ratio`).
3. **Supresión:** El rostro deja de ser un objeto suelto para convertirse en un atributo de la persona.

Esto genera una `ConsolidatedObservation`, la señal pura que marca el fin de la Percepción y el inicio del Control.

### 6. El Corazón de la Decisión: mana-control y el Scan Loop

Entramos en el dominio del Control. El **scan() loop** es el latido del sistema: ocurre cada **200ms** sin falta. Esta frecuencia fija es vital para la estabilidad de los cálculos temporales.

En cada tick, el sistema sigue este orden:

1. **Envejecer datos (**`**age_input**`**):** El sistema mira el reloj. Si los datos son más antiguos que el umbral `data_stale_ms`, el sistema entra automáticamente en un estado de **Blind (Ciego)** para proteger la lógica de decisiones basadas en pasado rancio.
2. **Predecir movimiento (Kalman):** Estima dónde deberían estar los objetos basándose en su velocidad previa.
3. **Determinar presencia:** Confirma si lo detectado es una entidad real o solo ruido pasajero.

### 7. Identidad y Espacio: Tracking, Zonas y Ocupación

Para que una detección aislada se convierta en una identidad estable, usamos herramientas matemáticas de alto nivel explicadas con lógica simple:

- **Kalman7:** Un filtro que gestiona la incertidumbre. Su superpoder es manejar **oclusiones**; si alguien pasa detrás de una columna (objeto oculto), Kalman7 predice dónde saldrá.
- **Algoritmo Húngaro:** Imagínelo como un **maître d'** de un restaurante que asigna cada camarero (nueva detección) a la mesa correcta (identidad existente) para minimizar la confusión y el esfuerzo.
- **Hysteresis (Histéresis):** Un "seguro de calma" que evita el parpadeo de señales (ej. decir que alguien entró y salió diez veces en un segundo si está parado en el umbral de una puerta).

Esto nos permite definir la **RoomCardinality** (Venciendo el caos: de `Empty` a `Single` o `Multiple`) y evaluar **Zonas** AABB que disparan eventos de "Ocupado" o "Vacante".

### 8. El Director de Orquesta: Motor FSM (Máquina de Estados)

El **FsmEngine** toma todas las señales anteriores y decide el estado lógico del sistema. Utiliza **Guards** (Guardias) que validan condiciones y **Dwell Timers** (Temporizadores de confirmación) para asegurar que un cambio de estado es real y no un error momentáneo.

El motor tiene dos roles obligatorios de seguridad: **Reset** (el punto de partida) y **Safe** (el refugio de emergencia cuando el sistema está "ciego" por falta de datos).

|   |   |
|---|---|
|Señal de Entrada|Estado de Salida|
|Persona en zona cama + Dwell 3s|**In_Bed**|
|Cardinalidad "Empty"|**Searching**|
|Datos vencidos (Stale)|**Safe** (Estado de seguridad)|

### 9. La Salida: Observabilidad y Señales de Estado

El viaje termina en el **FanoutObserver**, que reparte la inteligencia generada:

- Se generan logs **JSONL** para el historial clínico y técnico.
- Se envía información a **Rerun** para visualizar en tiempo real lo que la IA "ve" y "piensa".

Todo este conocimiento se sintetiza en la **SceneSignalsSnapshot**. No es solo un resumen; es un **vocabulario de tipos seguros** (el lenguaje compartido del sistema) que garantiza que cada cambio de estado y cada señal registrada sea coherente, libre de errores y lista para el análisis humano. El frame ha completado su viaje: de un simple impulso eléctrico a una decisión inteligente y confiable.