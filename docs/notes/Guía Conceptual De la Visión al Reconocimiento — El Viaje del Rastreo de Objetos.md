# Guía Conceptual: De la Visión al Reconocimiento — El Viaje del Rastreo de Objetos

## 1. Introducción: Ver no es lo mismo que Recordar

Bienvenido a esta exploración técnica. Como diseñador instruccional y experto en visión clínica, mi objetivo es guiarle en la transición de la **percepción estática** al **entendimiento temporal**.

En el desarrollo de **Mana Lite**, hemos establecido una distinción fundamental: para una máquina, "ver" es procesar una cuadrícula de píxeles; "entender" es reconocer la presencia humana a lo largo del tiempo. A diferencia de los sistemas de IA convencionales que operan de forma asíncrona y errática, Mana Lite se rige por un **PLC Superloop (ADR-003)**. Este ciclo síncrono y determinante garantiza que la lógica clínica sea predecible: cada fase (Ingest, Infer, Track, FSM) se completa antes de iniciar la siguiente, eliminando las condiciones de carrera (race conditions) que suelen plagar a la visión por computadora.

Este pipeline comienza descomponiendo la imagen cruda en niveles de abstracción, transformando datos efímeros en identidades con memoria.

## 2. La Anatomía de una Entidad: Los Tres Niveles de Percepción

Para que el sistema actúe con seguridad clínica, debemos diferenciar entre una detección puntual y una entidad con historia clínica. Basándonos en los **ADR-012, 017 y 018**, estructuramos la percepción en tres estados:

|   |   |   |   |
|---|---|---|---|
|Concepto|Definición Técnica|¿Tiene Identidad?|Ejemplo Clínico|
|**Detección**|Salida efímera del modelo (YOLO). Coordenadas (bboxes) en un solo frame.|**No.** Desaparece al siguiente ciclo.|Un cuadro que rodea a un "paciente" solo durante un I-frame.|
|**Observación Consolidada**|Fusión espacial de evidencias (Persona + Cara + Pose) en el mismo instante.|**No.** Es una "foto" completa sin pasado ni futuro.|El sistema entiende que los puntos de pose pertenecen al cuerpo detectado en esa posición.|
|**Entidad Rastreada**|Identidad persistente con un `track_id` único asignado por el Tracker.|**Sí.** El sistema la reconoce a través del tiempo.|El "Sujeto #101" cuya persistencia permite reglas FSM como **"Patient Exit"** al cruzar una zona.|

**Transición Crítica:** Para evolucionar de una observación a una entidad, el sistema requiere un mecanismo de **memoria** para validar el pasado y una capacidad de **predicción** para anticipar el futuro.

## 3. El Filtro de Kalman: El Pronosticador del Movimiento

Imagina que sigues a un paciente y este camina detrás de una cortina. Tu cerebro proyecta su trayectoria. En Mana Lite, este "Asistente de Navegación" es el **Filtro de Kalman (7D)** (**ADR-013**).

Actuando como una **"Sombra Fantasma"**, el Filtro de Kalman no espera pasivamente a ver al paciente; predice dónde _debería_ estar basándose en un modelo de velocidad constante. Esto permite que el sistema mantenga la atención incluso cuando el sensor sufre de "flicker" o ruido visual.

### El Vector de Estado 7D

El sistema utiliza 7 dimensiones para definir a cada entidad:

- **4 Elementos Medidos (Posición y Forma):**
    - Centro horizontal (`cx`) y vertical (`cy`).
    - Escala o área (`s`).
    - Ratio de aspecto o proporción (`r`).
- **3 Elementos Imaginados (Velocidades):**
    - Velocidad de movimiento horizontal (`dcx`).
    - Velocidad de movimiento vertical (`dcy`).
    - Velocidad de cambio de tamaño (`ds`).

Una vez proyectada esta "Sombra Fantasma", el sistema debe confirmar si las nuevas detecciones de la cámara coinciden con sus predicciones.

## 4. El Algoritmo Húngaro: El Asignador de Etiquetas

Para resolver la identidad, utilizamos el **Algoritmo Húngaro (Kuhn-Munkres)**. Piense en él como el **"Organizador de Protocolo en una Boda"**: tiene sillas marcadas (las predicciones de la Sombra Fantasma) e invitados que llegan (detecciones nuevas). Su trabajo es sentar a cada invitado en la silla correcta minimizando el error global.

### La Métrica del Costo: IoU

El puente entre la predicción y la realidad es el **IoU (Intersection over Union)**. El IoU mide el solapamiento entre el cuadro donde esperábamos ver al fantasma y el cuadro donde la cámara realmente vio a la persona.

1. **Costo de Asociación:** Se define como `1 - IoU`. Si el solapamiento es total, el costo es 0.
2. **Asignación Global:** El sistema calcula la matriz de costos para todas las personas en la habitación.
3. **Filtrado de Seguridad:** Si una detección está demasiado lejos de cualquier predicción (IoU < 0.2), el organizador la rechaza como un "invitado nuevo" y le asigna un `track_id` inédito.

## 5. El Poder de la Persistencia: Eficiencia y "Ghost Mode"

En clínica, la persistencia no es solo una solución para la oclusión (un paciente tras un sillón); es una herramienta de **Eficiencia Extrema**. Según el **ADR-007**, Mana Lite utiliza **I-Frame Gating**, ejecutando inferencia pesada solo en los cuadros clave (keyframes).

### El Beneficio del 97%

Si una cámara emite a 30 FPS con un GOP de 30, Mana Lite solo procesa 1 cuadro por segundo para inferencia, ahorrando un **97% de recursos de CPU/GPU**. Durante los 29 cuadros restantes, el sistema entra en **"Ghost Mode"**: el Filtro de Kalman sigue actualizando la posición de los `track_ids` mediante pura matemática, manteniendo el sistema receptivo sin gastar energía.

### Escenario: El Paciente Ocluido

1. **Visibilidad:** El paciente es seguido como `track_id: 101`.
2. **Oclusión/Gating:** El paciente desaparece tras un mueble o simplemente no hay un I-frame disponible.
3. **Persistencia:** Gracias al parámetro `max_age` (ADR-013), el `track_id` se mantiene vivo hasta por **40 segundos** (20 frames a 0.5 i-frames/seg) sin evidencia visual.
4. **Re-adquisición:** Al reaparecer, la nueva detección coincide con la "Sombra Fantasma" y la identidad se conserva, evitando falsas alarmas de "habitación vacía".

## 6. Contar con Inteligencia: Cardinalidad y Ocupación

El rastreo transforma el conteo de bboxes en la **Cardinalidad de la Habitación**. Para gestionar esto, implementamos la **Máquina de Estado de Ocupación (OSM)** (**ADR-014, ADR-026**), que utiliza histéresis temporal para filtrar el "sensor jitter".

```toml
# Estados de la Máquina de Ocupación (OSM)
[occupancy_policy]
Vacio              # Sin entidades confirmadas
Ocupación_Simple   # Una persona (tras 'single_confirm_ms')
Ambiguo            # +1 persona detectada (esperando confirmación)
Ocupación_Múltiple # Confirmado +1 persona (tras 'multiple_confirm_ms')
```

La histéresis (ej. `empty_confirm_ms`) asegura que si un paciente sale del cuadro por un segundo, el sistema no reporte "Habitación Vacía" instantáneamente, manteniendo la estabilidad del reporte clínico.

## 7. Conclusión: La Visión que Comprende

La arquitectura de Mana Lite convierte las matemáticas del Filtro de Kalman y el Algoritmo Húngaro en herramientas de cuidado humano. Al adherirse al **PLC Superloop (ADR-003)**, garantizamos tres pilares:

1. **Determinismo:** Al ser síncrono, las mismas entradas visuales siempre producen el mismo resultado, eliminando errores aleatorios.
2. **Eficiencia:** El "Ghost Mode" permite una vigilancia 24/7 en dispositivos de borde con un consumo mínimo.
3. **Seguridad Clínica:** La persistencia de identidad asegura que el sistema no "olvida" al paciente, permitiendo que la IA actúe como un observador atento y confiable.

En última instancia, estas fórmulas son el lenguaje que permite a la máquina recordar, cuidar y, finalmente, proteger.