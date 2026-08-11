# El Viaje del Dato: De un Píxel RTSP a la Inteligencia Semántica

Bienvenido a este recorrido por las entrañas de **mana-lite**, un sistema de visión artificial de alta fidelidad diseñado para transformar flujos de video desordenados en verdades semánticas procesables. Como mentor, mi objetivo es iluminar la lógica que permite que un bit en la red se convierta en una decisión clínica que salva vidas.

## 1. Introducción: La Anatomía de la Percepción y el Control

Para comprender la arquitectura de **mana-lite**, debemos visualizarla como un organismo digital con dos hemisferios interconectados pero especializados: la **Percepción** y el **Control**.

"Imagina que eres un observador en una habitación a oscuras. Tu ojo (la cámara) recibe ráfagas de luz. El hemisferio de la **Percepción** realiza la **extracción de características**: identifica que un destello es una 'persona' y otro es una 'cama'. Sin embargo, es el hemisferio del **Control** el que realiza la **inferencia de estado temporal**: entiende que si esa persona permanece en la cama por diez minutos, estamos ante un estado de 'descanso' o 'alerta'. Ver es un evento instantáneo; comprender es una narrativa constante."

La diferencia fundamental radica en que la detección es una foto fija de la realidad, mientras que el estado semántico es la interpretación lógica y estable de esa realidad a través del tiempo.

## 2. La Puerta de Entrada: Ingest Engine y la Selección de la Realidad

El viaje comienza en el `Ingest Engine` a través del `RetinaReader`. Este componente gestiona el protocolo RTSP y se enfrenta a un flujo masivo de paquetes H.264. En este punto, la prioridad es la eficiencia y la eliminación de la latencia.

El sistema aplica una política de **freshest keyframe** (fotograma clave más reciente). A diferencia de un reproductor de video convencional, no nos interesan los P-frames (que solo contienen cambios parciales), sino los **IDR (Instantaneous Decoder Refresh)**, que contienen la imagen completa. Para evitar procesar datos estáticos, el motor utiliza un **64-bit DefaultHasher** que suprime fotogramas duplicados si la imagen de la cámara se congela, ahorrando ciclos de GPU preciosos.

|   |   |   |
|---|---|---|
|Dato Crudo (RTSP)|Dato Útil (FrameBuffer)|¿Por qué se selecciona/descarta?|
|**P-frames**|Descartado|Requieren reconstrucción temporal y consumen CPU sin aportar una imagen base completa para la IA.|
|**IDR antiguos**|Descartado|Se eliminan para garantizar que la inferencia trabaje con la realidad de hace milisegundos, no segundos.|
|**Freshest IDR**|**Seleccionado**|Es la representación íntegra de la escena. Pasa por el `SoftwareDecoder` (FFmpeg) para convertirse en un buffer RGB.|

## 3. El Microscopio Inteligente: Infer Engine y la Cascada de Modelos

Con la imagen en memoria, el `Infer Engine` entra en acción ejecutando modelos YOLO. No lo hace de forma aislada, sino mediante **Blueprints** que definen **Cascadas** de ejecución. Esta arquitectura es una obra maestra de eficiencia matemática:

1. **Modelo Raíz (Root):** Analiza el fotograma completo buscando entidades generales (ej. "persona").
2. **Reglas de Cascada (Cascade Rules):** Si el modelo raíz confirma una presencia, se activan los modelos hijos.
3. **Cultivo Dinámico (Dynamic Cropping):** En lugar de re-procesar los 2MP de la imagen completa para ver un rostro, el sistema realiza un recorte (crop) alrededor de la entidad detectada. Esto reduce drásticamente la resolución de entrada para el modelo hijo (ej. 320x320), permitiendo una inferencia de alta fidelidad con un coste computacional mínimo.

## 4. Unificando la Visión: Detection Consolidator

Cuando múltiples modelos observan la misma escena, pueden generar ruidos o duplicidades. El `DetectionConsolidator` actúa como el "juez" de la percepción para generar una `ConsolidatedObservation` coherente.

- **Fusión IoU:** Utiliza el umbral `same_class_iou` para fusionar detecciones del mismo objeto provenientes de distintos modelos.
- **Anclaje de Componentes:** Mediante el parámetro `face_component_coverage`, el sistema determina si un rostro pertenece a un cuerpo específico. Si la cara está en la porción superior (validado por `face_max_center_y_ratio`), se "ancla" a la persona como un componente, en lugar de tratarla como una entidad independiente.

🟢 **Salida de Percepción:** El resultado final es un `ClinicalSample`, un paquete que contiene las observaciones consolidadas y la salud de la inferencia, listo para cruzar el puente hacia el Control.

## 5. El Pulso del Sistema: El Ciclo `scan()` y el dominio de Control

Aquí abandonamos el ritmo variable de la cámara para entrar en el determinismo del **Control**. El ciclo `scan()` opera a una cadencia fija (normalmente cada 200ms), consumiendo un `ProcessImage` (el `ClinicalSample` "congelado" para esa iteración).

El uso de un `ScanTimeline` propio ofrece beneficios críticos:

- **Determinismo Temporal:** Al no depender del reloj del sistema, las decisiones son inmunes a saltos de NTP o desfases del reloj de hardware.
- **Estabilidad de Señal:** Los temporizadores de permanencia (dwell) son absolutos, garantizando que "3 segundos" signifiquen lo mismo independientemente de la carga del procesador.
- **Inmunidad a Latencia:** Si la inferencia se retrasa, el control sigue latiendo, permitiendo una degradación elegante del sistema.

## 6. Identidad y Espacio: Tracker, Zones y Signals

Para que el sistema tenga "memoria" y no olvide a alguien por un parpadeo del sensor, empleamos el `Tracker`. Este utiliza el **Algoritmo Húngaro (Kuhn-Munkres)** para la asociación de datos, encontrando el coste mínimo global para asignar identidades y evitando emparejamientos erróneos.

La persistencia se logra mediante un filtro de Kalman de 7 dimensiones (**Kalman7**), cuyo vector de estado (cx, cy, s, r, \dot{cx}, \dot{cy}, \dot{s}) permite predecir la posición y escala de una persona incluso durante oclusiones momentáneas.

|   |   |   |
|---|---|---|
|Entidad de Código|Concepto Humano|Propósito Semántico|
|**Tracker (Kalman7)**|Identidad Persistente|Evita el _flickering_ de IDs usando predicción constante de velocidad y área.|
|**ZoneId (AABB)**|Geometría de Interés|Define áreas (Cama, Baño) para activar señales espaciales con histéresis.|
|**SignalTag**|Vocabulario Lógico|Traduce la física en variables: `persona.presente`, `cara.confianza`, etc.|

_Los valores de señal (_`_SignalValue_`_) pueden ser_ _**Bool**__,_ _**Count**__,_ _**Ratio**_ _(0.0-1.0) o_ _**Label**__._

## 7. El Cerebro Lógico: FSM Engine y el Estado Semántico Final

El viaje culmina en la Máquina de Estados Finat (FSM). Este motor consume las señales procesadas y decide el estado clínico de la habitación. Para garantizar la seguridad, la FSM utiliza **Guards** (condiciones lógicas) y **Dwell Timers** (tiempos de confirmación).

Un componente brillante es el `**face_was_inside**` **latch**: un pestillo lógico que permite que el sistema "recuerde" que vio la cara del paciente incluso si este gira la cabeza por unos segundos, manteniendo la continuidad del estado de monitoreo.

**Lógica de Transición (Pseudocódigo):**

🔵 **SI** (`persona_en_zona_cama` == TRUE) 🔵 **Y** (`face_was_inside` == TRUE) 🟡 **DURANTE** (Dwell 3000ms) 🟣 **ENTONCES** → Estado Final: **"Paciente_En_Cama_Identificado"**.

## 8. Conclusión: De Píxeles a Decisiones Clínicas

Lo que comenzó como un flujo desordenado de paquetes de red H.264 ha sido decodificado, filtrado, analizado por cascadas de redes neuronales, estabilizado mediante filtros de Kalman de 7 dimensiones y validado por una lógica de estados determinista.

La elegancia de **mana-lite** reside en su capacidad de transformar el caos de los píxeles en **Inteligencia Semántica**. Hemos convertido la luz capturada por un sensor en una señal digital robusta que no solo ve, sino que comprende, protegiendo la integridad de quienes más lo necesitan con el rigor de una arquitectura de misión crítica.