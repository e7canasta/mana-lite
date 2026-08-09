# El Viaje del Dato: Del Video a la Inteligencia en Tiempo Real

Como arquitecto de sistemas de visión computacional y diseñador instruccional, mi misión es desglosar la complejidad de **Mana Lite**. Este sistema no es simplemente un consumidor de video; es una refinería de datos de alta precisión que transforma flujos de red ruidosos en decisiones clínicas deterministas. A continuación, exploraremos la ingeniería detrás de este proceso.

## 1. Introducción: El Concepto de Mana Lite

Mana Lite se aleja de las arquitecturas tradicionales multi-proceso para adoptar una **Arquitectura de Binario Único** (ADR-001). Mientras que otros sistemas sufren por la complejidad de la comunicación entre procesos (IPC) y condiciones de carrera, Mana Lite se ejecuta como un solo binario estático.

"Simplicidad sobre complejidad para dispositivos de borde."

Para garantizar la seguridad clínica, el sistema utiliza un **Superloop** síncrono. Sin embargo, existe un matiz arquitectónico crucial: aunque la lógica principal es de un solo hilo, el sistema emplea un **runtime de Tokio (current-thread)** para gestionar el "drain" asíncrono de los sockets de red. Esta elección permite manejar la entrada de video de forma no bloqueante sin sacrificar el determinismo del análisis. El riesgo asumido es la falta de aislamiento de fallos: un _segfault_ en la inferencia detiene todo el binario, un compromiso aceptable para la eficiencia en el borde.

## 2. La Puerta de Entrada: Ingesta mediante RTSP

La primera fase es la negociación con la cámara mediante el protocolo **RTSP**. Aquí, Mana Lite divide responsabilidades de forma quirúrgica entre la red y el procesamiento de imagen (ADR-004).

|   |   |   |
|---|---|---|
|Responsabilidad|Herramienta: `retina` (Red y Protocolo)|Herramienta: `FFmpeg` (Pixeles)|
|**Protocolo**|Gestión de sesión (DESCRIBE, SETUP, PLAY) y _keepalives_.|N/A|
|**Capa de Red**|Depaquetización RTP, rastreo de secuencias y pérdida de paquetes.|Decodificación H.264/H.265 a buffers RGB.|
|**Filtrado NAL**|Inspección de tipos de unidad NAL sin decodificar pixeles.|Gestión de memoria y alineación de stride para la IA.|

Esta separación permite que el sistema detecte si un paquete es útil antes de gastar un solo ciclo de CPU en transformarlo en imagen.

## 3. El Filtro de Eficiencia: ¿Qué es un I-Frame y por qué importa?

En un flujo de video estándar, la mayoría de los cuadros son **P-frames** (solo diferencias respecto al anterior). Procesar cada cuadro a 30 FPS saturaría cualquier dispositivo de borde. Mana Lite implementa **"I-Frame Gating"** (ADR-007) a nivel de la capa de abstracción de red (**NAL**).

Al inspeccionar el flujo antes de la decodificación, el sistema ignora sistemáticamente los P-frames y solo decodifica los **I-frames** (puntos de acceso aleatorio) que aparecen al inicio de cada **GOP** (Group of Pictures).

- **Ahorro Masivo:** En una cámara a 30 FPS con un GOP de 30, el gating a nivel de NAL permite **omitir 29 decodificaciones de cada 30**. Esto reduce el uso de GPU y CPU entre 30 y 60 veces.
- **Determinismo Clínico:** En entornos hospitalarios, los eventos críticos (como una salida de cama) ocurren en segundos, no milisegundos. Analizar un I-frame cada 1-2 segundos es suficiente y reduce el ruido visual.
- **Decodificación Selectiva:** El sistema solo paga el costo de decodificación cuando sabe que tiene un cuadro completo y fresco entre manos.

## 4. El Corazón del Sistema: El Superloop de 7 Fases

Inspirado en los **PLC** (Controladores Lógicos Programables), el sistema ejecuta un bucle síncrono y secuencial dividido en 7 fases (ADR-003). Este diseño garantiza un tiempo de ejecución del ciclo (WCET) predecible y evita las inconsistencias de los sistemas asíncronos.

1. **TIMERS**: Actualiza relojes de sistema para medir duraciones exactas.
2. **EVALUATE**: Evalúa temporizadores de dwell para seguridad clínica.
3. **INGEST**: Drena el socket de red y extrae el I-frame más reciente.
4. **INFER**: Ejecuta la inferencia de los modelos (YOLO, Depth, etc.).
5. **ZONES**: Cruza las detecciones con las regiones espaciales definidas.
6. **FSM**: La Máquina de Estados Finitos procesa las reglas de transición.
7. **PUBLISH**: Realiza un **flush atómico** de eventos en formato JSONL para evitar datos corruptos.

## 5. De Pixeles a Datos Útiles: Inferencia, Tracking y FSM

El recorrido del dato no es directo; es un proceso de refinamiento en capas (ADR-013, ADR-017, ADR-018).

**Flujo de transformación:** `Pixeles -> Inferencia (YOLO) -> Consolidación (Fusion) -> Tracking (SORT) -> Zonas -> FSM`

- **Consolidación:** Si ejecutamos varios modelos (ej. Person + Face), esta capa fusiona las evidencias. Una "cara" se asocia a una "persona" para crear una **Observación Consolidada** única por sujeto.
- **Tracking (SORT):** Aquí es donde las detecciones efímeras ganan identidad. Usamos un **Filtro de Kalman de 7 dimensiones** para predecir el movimiento y el **Algoritmo Húngaro** para asociar detecciones nuevas con identidades existentes (**track_id**).
- **Ghost Mode:** Dado que solo procesamos I-frames, el sistema entra en "Modo Fantasma": mantiene la última posición conocida y sigue evaluando temporizadores de zonas aunque no haya una nueva inferencia, siempre que no se supere el límite `data_stale_ms`.
- **FSM:** El cerebro final evalúa "Guards" (ej. `zone_occupied` + `dwell_min`). Solo si las condiciones se cumplen durante un tiempo clínico seguro, se emite una transición.

## 6. Conclusión: La Magia de la Optimización

La arquitectura de Mana Lite es un triunfo de la ingeniería de borde sobre la fuerza bruta. Al mover el filtrado a la capa de red y estructurar el procesamiento en un loop determinista, logramos que un hardware modesto realice tareas de grado clínico.

**Takeaways para el despliegue:**

- 🚀 **Frescura de Datos:** El sistema siempre ignora el video viejo acumulado en buffers, analizando solo lo que sucede "ahora".
- 🛡️ **Estabilidad:** El diseño síncrono y el binario único eliminan las condiciones de carrera y simplifican el mantenimiento (un solo servicio de `systemd`).
- 🌡️ **Eficiencia Térmica:** El gating de I-frames evita el sobrecalentamiento al reducir drásticamente la carga de decodificación y de inferencia, extendiendo la vida útil del hardware.