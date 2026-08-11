# Manual de Procedimientos: Definición de Estados de Ocupación Clínica y Lógica de Estabilización Temporal

## 1. Introducción y Marco Estratégico de la Operación

En el despliegue de soluciones de inteligencia artificial para entornos de cuidados críticos, el sistema `mana-lite` se erige como una infraestructura de misión crítica, diseñada para transformar flujos de video masivos en inteligencia operativa. La transformación de "detecciones crudas" —simples coordenadas rectangulares— en "estados semánticos" (ej. "Paciente en Cama") es el imperativo estratégico para mitigar la fatiga por alarmas y garantizar la seguridad del paciente. Sin esta capa de abstracción, el personal clínico se vería saturado por ruido visual y falsos positivos, comprometiendo la fidelidad del monitoreo.

La arquitectura de `mana-lite` se fundamenta en el desacoplamiento estricto entre **Percepción** y **Control**. Mientras la Percepción opera a una frecuencia variable dictada por el hardware de inferencia, el Control se rige por un motor determinista. Esta separación garantiza que la latencia inherente al procesamiento de redes neuronales no contamine la lógica de negocio, permitiendo que las decisiones clínicas se basen en una línea de tiempo estable y predecible.

### Objetivos Principales del Marco Operativo

- **Estabilidad:** Implementar filtros temporales que eliminen oscilaciones causadas por ruido visual o dropouts momentáneos.
- **Determinismo:** Asegurar que cada entrada sensorial resulte en un estado lógico previsible mediante el uso de máquinas de estado finitas (FSM).
- **Precisión Semántica:** Utilizar identificadores fuertemente tipados para garantizar que los datos generados correspondan exactamente a la realidad clínica del paciente.

La validez del sistema no reside únicamente en la potencia de sus modelos de visión, sino en la integridad de sus dominios de datos y la robustez de su lógica de estabilización.

## 2. Arquitectura del Flujo de Datos: De la Percepción al Control Semántico

El ciclo de vida del dato comienza en el **Ingest Engine**, que gestiona la conectividad RTSP y decodifica paquetes H.264 priorizando IDR (keyframes) para minimizar la latencia. Una vez procesados por el **Infer Engine**, los resultados se consolidan en un `ClinicalSample`. Es aquí donde el sistema utiliza la crate `mana-id` para inyectar precisión semántica mediante **Identificadores Fuertemente Tipados** (`ModelId`, `ZoneId`, `StateId`). Este enfoque arquitectónico elimina errores catastróficos derivados de comparaciones de cadenas de texto (string-matching) en la lógica de control.

El sistema de control, `**mana-control**`, consume estas observaciones en un "tick" fijo de **200ms (5Hz)**. Para garantizar la inmunidad ante saltos de tiempo del sistema (NTP) o variaciones de reloj, se utiliza una `**ScanTimeline**` con `**ScanInstant**`. Este reloj monatónico es el único árbitro válido para los temporizadores de permanencia (dwell timers) y la lógica de envejecimiento de datos.

### Dominios de Operación y Propósito de Negocio

|   |   |
|---|---|
|Dominio de Código|Propósito de Negocio|
|**Ingest & Perception**|Ingesta de video, normalización de coordenadas y ejecución de cascadas de modelos YOLO para extraer evidencia visual.|
|**Control (mana-control)**|Transformación de evidencia en estados estables mediante Kalman, evaluación de zonas y ejecución de la lógica FSM determinista.|
|**Domain Vocabulary**|Implementación de `mana-id` para garantizar la seguridad de tipos y evitar errores de referencia en estados y zonas críticas.|
|**Observability**|Auditoría forense mediante registros JSONL y telemetría de salud del sistema para la validación post-incidente.|

Una vez establecido el flujo, el sistema aplica el primer mecanismo de defensa de datos: la política de presencia.

## 3. Política de Presencia (PresencePoiPolicy) y Validación de Identidad

La `**PresencePoiPolicy**` constituye el filtro primario contra falsos positivos. Define la lógica para validar a una **Persona de Interés (POI)**, asegurando que solo entidades humanas confirmadas afecten la toma de decisiones. En este dominio, el estado `**Ambiguous**` es fundamental: se utiliza específicamente para **suprimir el rastreo (tracking)** cuando la identidad no es clara, evitando la creación de "tracks fantasmales" originados por ruido de fondo.

### Parámetros de Seguridad y Protección del Paciente

- `**on_ms**`**:** Tiempo de confirmación requerido. Actúa como el umbral de confianza para ignorar artefactos visuales transitorios.
- `**off_ms**`**:** Este parámetro es el **amortiguador matemático contra reportes de "Falso Vacío"**. Garantiza que un paciente no sea marcado como ausente (unsupervised) simplemente porque un facultativo obstruyó momentáneamente la línea de visión del sensor.

### Reglas de Transición de Estados de Presencia

- **Absent:** Estado por defecto; ausencia de POI confirmada en la región de interés.
- **Present:** Transición activada una vez que la detección continua supera el umbral `on_ms`.
- **Ambiguous:** Estado de precaución activado durante la pérdida de señal; retiene la presencia lógica durante `off_ms` antes de revertir a _Absent_, protegiendo la continuidad del reporte.

La confirmación de presencia es el insumo crítico que alimenta la máquina de estados de ocupación para determinar la cardinalidad de la habitación.

## 4. Política de Ocupación (OccupancyPolicy) e Histéresis Clínica

La política `**OccupancyPolicy**` traduce los rastros confirmados en estados de habitación: `Empty`, `Single` y `Multiple`. Para evitar el "chattering" (oscilación rápida de estados), el sistema aplica **Histéresis**, un retardo de estado dependiente que previene cambios lógicos ante fluctuaciones marginales de la detección.

El parámetro `**require_confirmed_tracks**` es vital para la integridad clínica: al activarlo, el sistema ignora cualquier detección "tentativa" (ruidosa), asegurando que las alertas de ocupación múltiple o habitación vacía se basen únicamente en identidades validadas por el sistema de confianza.

### Umbrales Técnicos de Histéresis de Ocupación

|   |   |
|---|---|
|Parámetro|Propósito de Control Clínico|
|`single_confirm_ms`|Validación temporal antes de declarar que un paciente está solo en la unidad.|
|`empty_confirm_ms`|Retardo de seguridad para confirmar que la habitación ha sido desalojada completamente.|
|`multiple_confirm_ms`|Umbral crítico de validación antes de disparar una alerta de presencia de múltiples personas.|

Para mantener la continuidad de estos estados en escenas complejas, el sistema recurre a modelos matemáticos de predicción.

## 5. Mecanismos de Estabilidad: Rastreo Kalman y Algoritmo Húngaro

Para gestionar oclusiones y suavizar el jitter, `mana-lite` implementa un rastreador basado en el filtro **Kalman7 (7-dimensional state vector)**. A diferencia de filtros básicos, el vector [cx, cy, s, r, \dot{cx}, \dot{cy}, \dot{s}] modela específicamente la escala (s) y el aspecto (r), lo que hace al sistema excepcionalmente **resiliente a la perspectiva variable de las cámaras montadas en pared** en entornos hospitalarios.

La asociación de datos se resuelve mediante el **Algoritmo Húngaro (Kuhn-Munkres)**, que realiza una minimización del costo global de asignación. Esto evita que el sistema "intercambie" la identidad de un paciente con un médico cuando sus trayectorias se cruzan.

### Ciclo de Vida de un Track (Persistencia de Identidad)

1. **Tentative:** Rastro en fase de acumulación de confianza; no afecta la ocupación.
2. **Confirmed:** Identidad activa y vinculada a la lógica de negocio clínica.
3. **Ghosting:** Fase de **extrapolación matemática**. Si el detector visual falla, el filtro Kalman predice la posición del paciente, manteniendo el estado de ocupación correcto hasta que se recupere la señal o expire el tiempo de vida máximo.

Esta estabilidad permite al sistema ejecutar reglas espaciales complejas mediante geometría semántica.

## 6. Geometría Semántica y Motores de Reglas (Zones & FSM)

El **ZoneEngine** define regiones geométricas (AABB) con significado clínico (ej. "Zona de Cama"). La interacción entre los tracks confirmados y estas zonas genera señales que alimentan la **Máquina de Estados Finitos (FSM)**. Siguiendo los principios de ingeniería de control de procesos, nuestra FSM incluye obligatoriamente los roles `**safe**` y `**reset**`. El rol `safe` es el estado de repliegue ante la pérdida de integridad de datos, garantizando que el sistema no emita decisiones erróneas durante fallos de red.

### Lógica de Guardias y Latches de Continuidad

Para garantizar reportes limpios en los registros JSONL, la FSM utiliza **Guards** y el `**face_was_inside**` **latch**. Este último es un mecanismo de sofisticación arquitectónica que mantiene la continuidad del estado incluso si el detector de rostros pierde la señal por unos frames, siempre que el track permanezca dentro de la zona.

### Resumen de Lógica de Guardias (FSM Guards)

- **ZoneOccupied:** Disparo basado en la intersección de un track confirmado con una zona.
- **DataStale:** Guardia de seguridad que fuerza la transición al estado `safe` si la latencia de percepción excede los límites permitidos.
- **Signal:** Comparación directa en la `SignalTable` (ej. `persona.presente == true`).
- **DepthRule:** Evaluación de umbrales de profundidad 3D (ej. distancia mediana < 1.5m) para confirmar aproximaciones a la cama.

## 7. Validación Operativa y Telemetría de Salud

El sistema culmina con una capa de protección de salud que monitoriza la integridad del flujo. Los estados `**Blind**` **(Ciego)** y `**Stale**` **(Caducado)** no son simples métricas de telemetría; son **guardias de seguridad proactivos** que inhabilitan la toma de decisiones automatizadas si el sensor falla o los datos están desactualizados, previniendo errores clínicos con consecuencias vitales.

### Niveles de Registro y Auditoría JSONL

El `MetricsEngine` genera un log estructurado JSONL compatible con auditorías clínicas, filtrado por severidad:

|   |   |   |
|---|---|---|
|Nivel de Log|Eventos Registrados|Aplicación Clínica|
|**Debug**|Inferencia por frame, latencia de modelos y métricas de Kalman.|Diagnóstico técnico y calibración inicial.|
|**Info**|Transiciones FSM, eventos de ocupación y cambios en zonas.|Auditoría operativa y seguimiento de eventos del paciente.|
|**Quiet**|Errores críticos de hardware y cambios de configuración.|Entornos de producción de alto rendimiento.|

La configuración meticulosa de estas políticas y umbrales es el cimiento de la confianza clínica, permitiendo que la IA actúe como un vigía incansable y preciso en la seguridad del paciente.