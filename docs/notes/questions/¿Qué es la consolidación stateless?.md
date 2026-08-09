La **consolidación stateless** es el modo operativo base de Mana Lite en el que cada cuadro de video se procesa de forma independiente para producir una observación consolidada, sin mantener una **identidad temporal** entre frames.

A continuación se detallan sus características y funcionamiento según las fuentes:

### Definición y Propósito

- **Sin Memoria Temporal:** En este modo, el sistema no utiliza `track_id`, entidades persistentes ni memoria entre cuadros; cada frame produce detecciones por modelo y una observación única que representa exclusivamente lo que ocurre en ese instante.
- **Fase de Calibración:** Se recomienda utilizar la consolidación stateless para validar la precisión de los detectores y la lógica espacial (como las zonas) antes de activar el seguimiento temporal (tracking), evitando así el ruido o la deriva que podrían introducir los filtros de Kalman en etapas de ajuste inicial.

### Mecanismo de Fusión

El proceso ocurre en la fase de **Consolidate** del pipeline, donde las detecciones de múltiples modelos se integran en una estructura denominada `ConsolidatedObservation`:

- **Fusión por IoU:** Las detecciones de la misma clase (por ejemplo, "persona" detectada por dos modelos distintos) se pueden fusionar mediante el cálculo de Intersección sobre Unión (IoU).
- **Asociación por Contención (Containment):** Los componentes secundarios, como los rostros, se asocian a la entidad primaria (persona) si su cuadro delimitador (_bbox_) está contenido dentro del de la persona, en lugar de usar IoU puro.
- **Enriquecimiento:** Modelos como los de pose o segmentación "enriquecen" la observación primaria de la persona con evidencia adicional (puntos clave o máscaras) sin crear entidades duplicadas en la escena.

### Salidas y Observabilidad

- **Eventos JSONL:** El sistema emite un evento de tipo `consolidated_detection`, que resume los hallazgos del frame actual pero no contiene identificadores de seguimiento.
- **Visualización en Rerun:** Las observaciones consolidadas se publican en la ruta `/world/camera/observations`. A diferencia del modo con tracking, las cajas delimitadoras en esta vista no tienen un `track_id` y pueden aparecer o desaparecer según la salida directa del detector en cada frame.
- **Independencia de Profundidad:** Es importante notar que los datos del modelo `depth-standard` no entran en el proceso de consolidación, ya que funcionan como una fuente de evidencia numérica independiente.

La **consolidación stateless** es el modo operativo base de Mana Lite en el que cada cuadro de video se procesa de forma independiente para producir una observación consolidada, **sin mantener una identidad temporal** entre frames. En este modo, el sistema no genera identificadores de seguimiento (`track_id`), entidades persistentes ni memoria entre cuadros; cada frame representa exclusivamente lo que ocurre en ese instante preciso.

Sus características y mecanismos principales son:

- **Fusión y Enriquecimiento:** Las detecciones de la misma clase (por ejemplo, "persona") pueden fusionarse mediante el cálculo de Intersección sobre Unión (IoU). Los modelos secundarios de pose, rostro (face) y segmentación "enriquecen" a la entidad primaria sin crear bboxes adicionales en la escena.
- **Asociación de Rostros:** El sistema utiliza una lógica de **asociación por contención** en lugar de IoU puro para los rostros; un rostro se asocia a una persona solo si su cuadro delimitador está contenido dentro del de la persona y su centro se encuentra en la mitad superior del cuerpo.
- **Propósito de Calibración:** Se recomienda utilizar este modo durante las fases de calibración para validar la precisión de los detectores y la lógica de zonas antes de introducir el rastreo temporal, evitando así posibles derivas o ruidos del filtro de Kalman.
- **Independencia de Profundidad:** Los datos provenientes del modelo `depth-standard` no entran en este proceso de consolidación, ya que funcionan como una fuente de evidencia numérica y visual independiente.

Desde el punto de vista de la **observabilidad**, la consolidación stateless se refleja en:

1. **JSONL:** Se emite un evento de tipo `consolidated_detection` que resume los hallazgos del cuadro actual sin ID de seguimiento.
2. **Rerun:** Las observaciones se publican bajo la ruta `/world/camera/observations`. A diferencia del modo con tracking, aquí las cajas delimitadoras no tienen ID y pueden aparecer o desaparecer según la salida directa del detector en cada frame.