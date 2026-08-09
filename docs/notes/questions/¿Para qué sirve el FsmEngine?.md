El **FsmEngine** es el componente central de la capa de toma de decisiones de alto nivel en el sistema Mana Lite. Su función principal es actuar como un **procesador de estados** que evalúa la lógica de negocio basándose en la evidencia recopilada por el resto del pipeline de visión.

A continuación se detallan sus propósitos y mecánicas principales:

### 1. Evaluación de Lógica de Negocio

El motor consume datos procesados (evidencia) de múltiples sub-sistemas, incluyendo el seguimiento de objetos (**tracking**), zonas de ocupación, reglas de profundidad y detección de rostros. Con esta información, determina estados complejos de la escena, como si una persona está "en cama" o realizando un "egreso".

### 2. Gestión de Transiciones de Estado

El `FsmEngine` es responsable de:

- **Mantener el estado actual:** Rastrea en qué modo operativo se encuentra el sistema (ej. `idle`, `searching`, `detected` o `alert`).
- **Evaluar transiciones:** Compara los resguardos (**FsmGuard**) definidos en la configuración contra el contexto de la escena actual (`FsmSceneContext`) para decidir cuándo cambiar de un estado a otro.
- **Soportar saltos globales:** Permite el uso de **Wildcard Transitions** (transiciones comodín) para saltar a estados críticos, como `blind` o `error`, desde cualquier punto si se detecta, por ejemplo, una pérdida de señal de video.

### 3. Control Dinámico del Pipeline

Una de las capacidades más potentes del `FsmEngine` es que permite al sistema **ajustar su comportamiento dinámicamente**. Dependiendo del estado activo, el motor puede determinar qué modelos de IA deben estar ejecutándose en ese momento, permitiendo pasar de una detección de alta velocidad a una estimación de pose de alta precisión solo cuando sea necesario.

### 4. Manejo de Lógica Temporal y "Latches"

El motor gestiona mecánicas complejas para evitar inconsistencias:

- **Dwell Timers:** Utiliza temporizadores de permanencia para asegurar que una condición sea estable antes de disparar una transición, evitando el "parpadeo" rápido entre estados.
- **Latching (Enganche):** Mantiene banderas internas como `face_was_inside`, que recuerda si se detectó un rostro previamente en una sesión, lo que ayuda a distinguir entre una salida normal y una pérdida momentánea del rastreo.

### 5. Interacción con la Configuración

El `FsmEngine` sirve de puente entre la configuración definida por el usuario en el archivo `fsm.toml` (el **FsmCatalog**) y la evidencia en tiempo real proveniente de cada frame de video. Antes de iniciar, el sistema valida que todos los resguardos que referencian zonas existan realmente en el catálogo de zonas.


El **FsmEngine** es el componente central de la capa de toma de decisiones de alto nivel en el sistema Mana Lite. Su propósito fundamental es actuar como un **procesador de estados** que transforma la evidencia técnica recopilada por el pipeline de visión en una comprensión semántica del comportamiento y los estados de la habitación.

Sus funciones y responsabilidades principales incluyen:

### 1. Evaluación de Lógica de Negocio

El motor determina estados complejos de la escena (como "persona en cama", "buscando cara" o "realizando un egreso") consumiendo datos procesados de diversos sub-sistemas, incluyendo el seguimiento de objetos (**tracking**), zonas de ocupación, reglas de profundidad y detección de rostros.

### 2. Gestión de Estados y Transiciones

El `FsmEngine` es responsable de mantener la coherencia operativa del sistema mediante:

- **Mantenimiento del Estado Actual:** Rastrea el modo operativo activo (ej. `idle`, `watching`, `searching`, `alert`).
- **Evaluación de Guards:** Compara los resguardos lógicos (condiciones de zonas, profundidad o tiempo) definidos en la configuración contra el contexto de la escena actual (`FsmSceneContext`) para decidir cuándo cambiar de estado.
- **Transiciones Globales (Wildcards):** Permite saltos inmediatos a estados críticos (como `blind` o `error`) desde cualquier otro estado si se activa un guard de alta prioridad, como la pérdida de señal de video (`data_stale`).

### 3. Control Dinámico del Pipeline

Una capacidad clave del `FsmEngine` es que permite al sistema **ajustar su comportamiento dinámicamente**. Dependiendo del estado activo en la FSM, el motor puede determinar qué modelos de IA deben estar ejecutándose, permitiendo, por ejemplo, pasar de una detección de personas de alta velocidad a una estimación de pose o reconocimiento facial de alta precisión solo cuando sea necesario.

### 4. Mecánicas de Estabilidad y Memoria

Para evitar inconsistencias y falsas alarmas, el motor gestiona:

- **Dwell Timers (Temporizadores de Permanencia):** Aseguran que una condición sea estable durante un tiempo mínimo configurado antes de disparar una transición, eliminando el "parpadeo" rápido entre estados.
- **Latching (Enganche):** Mantiene banderas internas como `face_was_inside`, que recuerda si se detectó un rostro previamente en la sesión actual, lo que permite distinguir entre una salida normal de la habitación y una pérdida momentánea del rastreo.

### 5. Integración en el Ciclo de Vida (Superloop)

El `FsmEngine` opera en fases específicas del **PLC superloop**:

- **EVALUATE:** Evalúa los guards contra las detecciones y estados de zona actuales.
- **FSM:** Ejecuta las transiciones confirmadas y avanza el estado del sistema.

Finalmente, el motor genera **eventos semánticos** (como alertas de salida de cama) y snapshots de estado que alimentan la capa de observabilidad en formato JSONL y la visualización en Rerun.