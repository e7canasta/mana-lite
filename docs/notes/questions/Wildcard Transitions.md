
¿Para qué sirven las Wildcard Transitions?

Las **Wildcard Transitions** (definidas en la configuración como `from = "*"`) son un mecanismo fundamental de la Máquina de Estados Finitos (FSM) de Mana Lite que permite al sistema saltar a un estado específico **desde cualquier otro estado actual** si se cumple una condición crítica.

Sus propósitos y características principales son los siguientes:

### 1. Gestión Global de Errores

Su uso más común es el **manejo de fallos críticos del sistema**, como la pérdida de la señal de video.

- **Estado `blind`:** Si el resguardo (_guard_) `data_stale` se activa (porque no se han recibido frames en un tiempo determinado), una transición comodín permite que el sistema pase inmediatamente a un estado de "ceguera" o error, sin importar si anteriormente estaba en `idle`, `searching` o `detected`.

### 2. Sobreescritura de Lógica de Negocio

También se utilizan para forzar estados cuando cambian las condiciones básicas de la escena que invalidan el flujo actual.

- **Ejemplo de Ocupación:** En blueprints diseñados para una sola persona (como `detect-room-face`), se puede configurar una transición comodín para volver al estado `idle` de forma inmediata si se detecta la presencia de múltiples personas, abortando cualquier lógica de seguimiento individual activa.

### 3. Mecánica de Evaluación

A diferencia de las transiciones normales que dependen del estado actual, las Wildcard Transitions se **evalúan en cada frame**, independientemente de dónde se encuentre la FSM en ese momento. Esto garantiza que el sistema siempre sea capaz de reaccionar a eventos de alta prioridad de manera determinista.

### 4. Observabilidad y Logs

En el sistema de registros forenses (JSONL) y en la lógica de diagnóstico, estas transiciones se identifican claramente:

- **Origen del Evento:** En los eventos de tipo `face_dwell` o `fsm`, el campo `source` se marca como **"wildcard"** para distinguir estos saltos globales de las evaluaciones normales basadas en evidencia de frames (`keyframe`).
- **Registro de Transición:** Aunque el salto sea global, el sistema sigue registrando los datos de la transición confirmada, incluyendo el activador (_trigger_) y el tiempo de permanencia si aplica.



---

Para configurar una **Wildcard Transition** (transición comodín) en el sistema Mana Lite, debes editar el archivo de configuración lógica de la Máquina de Estados, usualmente denominado **`fsm.toml`**.

Estas transiciones se definen utilizando el identificador especial **`from = "*"`**, lo que permite al sistema saltar a un estado específico desde **cualquier otro estado actual** si se cumplen las condiciones necesarias.

### Pasos para la configuración en `fsm.toml`

La estructura básica dentro del bloque de transiciones es la siguiente:

1. **Definir el origen:** Usa `from = "*"` para indicar que la transición es global.
2. **Definir el destino:** Indica en el campo `to` el estado al que debe moverse el sistema (por ejemplo, `blind`, `error` o `idle`).
3. **Configurar los Guards:** Añade una lista de condiciones (`guards`) que deben evaluarse como verdaderas para que la transición se dispare.
4. **Configurar el Dwell (Opcional):** Puedes añadir un parámetro `dwell` (en milisegundos) si deseas que los guards se mantengan estables durante un tiempo antes de ejecutar el salto.

### Ejemplo de configuración técnica

Un uso común es el manejo de la pérdida de señal de video mediante el guard `data_stale`:

```
[[fsm.transitions]]
from = "*"
to = "blind"
guards = [
    { type = "data_stale" }
]
```

### Características operativas clave

- **Evaluación constante:** A diferencia de las transiciones normales, las de tipo wildcard se **evalúan en cada frame**, independientemente del estado en el que se encuentre la FSM.
- **Prioridad y Resiliencia:** Son fundamentales para la gestión global de errores. Por ejemplo, permiten que el sistema entre en un estado de "ceguera" inmediatamente si el monitor de salud detecta que no están llegando frames (`Stale` o `Blind`).
- **Uso en Blueprints clínicos:** En perfiles como `detect-room-face`, se utilizan para abortar sesiones de seguimiento si cambian las condiciones básicas, como el paso de una sola persona a múltiples ocupantes en la sala.
- **Trazabilidad:** En los logs forenses (JSONL), estos eventos se registran con el campo `source = "wildcard"` para distinguirlos de las transiciones normales basadas en evidencia de frames (`keyframe`).

---

El **filtro de presencia** (`PresenceFilter`) en el sistema Mana Lite tiene como propósito fundamental actuar como una **capa de estabilización o "debounce"** que transforma las detecciones transitorias de cada cuadro en señales de presencia robustas y continuas. Su función principal es filtrar el ruido de los sensores y manejar las breves desapariciones (dropouts) del detector de IA.

Sus funciones y mecanismos clave son:

### 1. Estabilización de la Señal (On/Off Ticks)

El filtro utiliza un sistema basado en "ticks" (cuadros de inferencia) para confirmar el estado de un objeto, generalmente una persona:

- **Confirmación de entrada (On ms):** Define cuánto tiempo real acumulado de detección consecutiva se necesita antes de que el sistema considere a la persona oficialmente "presente" (`PresenceState::Present`). Esto evita falsos positivos por parpadeos momentáneos del modelo de IA.
- **Confirmación de ausencia (Off ms):** Cuánto tiempo real acumulado de la ventana de retención de ausencia antes de cambiar el estado a "ausente" (`PresenceState::Absent`).

### 2. Retención de Señal (Signal Holding)

Una de las funciones más críticas es "sostener" la última observación conocida durante fallas momentáneas de inferencia u oclusiones breves. Si el detector no encuentra a la persona en un cuadro específico, el filtro **mantiene la posición previa** durante el periodo real definido por `off_ms` (independiente de la cadencia de keyframes), evitando que el sistema pierda la continuidad o reinicie los contadores de la lógica de negocio innecesariamente.

### 3. Ubicación en el Pipeline

El filtro de presencia se sitúa estratégicamente entre la **Consolidación de Inferencia** y el **Tracker**. Esto asegura que la señal del "Sujeto de Interés" (POI) sea estable antes de que se intente asignar una identidad temporal o se disparen cambios en el estado de la habitación.

### 4. Diferencia con la Máquina de Ocupación

Es importante distinguir este filtro de la `OccupancyStateMachine`:

- **Filtro de Presencia:** Se basa en cuadros (ticks) y busca **estabilidad de señal** inmediata.
- **Máquina de Ocupación:** Se basa en tiempo real (milisegundos) y busca **estabilidad de estado** (determinar si la habitación está vacía, con una persona o con múltiples personas) aplicando una histéresis temporal más conservadora.

En resumen, el filtro de presencia se utiliza para que el sistema sea **resiliente ante la inestabilidad de los modelos de visión**, garantizando que un "parpadeo" del detector no rompa la lógica clínica o de seguimiento.


----

La configuración del **dwell timer** (temporizador de permanencia) se realiza principalmente en el archivo de lógica de la Máquina de Estados Finatarios, usualmente denominado **`fsm.toml`**. Este mecanismo actúa como un filtro temporal para evitar el "flickering" o parpadeo rápido entre estados, asegurando que los cambios solo ocurran cuando una condición es estable.

Existen dos tipos fundamentales de configuración de tiempo en la FSM:

### 1. Transition Dwell (`dwell`)

Este es el temporizador de **activación de transición**. Define cuánto tiempo (en milisegundos) deben cumplirse **continuamente** todos los resguardos (_guards_) de una transición antes de que esta se dispare.

- **Configuración:** Se define mediante el campo `dwell` dentro del bloque de una transición.
- **Ejemplo en `fsm.toml`:**
    
    ```
    [[fsm.transitions]]
    from = "searching"
    to = "detected"
    dwell = 500  # La cara debe detectarse por 500ms seguidos
    guards = [
        { type = "face_present" }
    ]
    ```
    
- **Mecánica:** El contador se incrementa en cada ciclo del superloop mientras los resguardos sean verdaderos. Si en algún frame un resguardo falla, el temporizador se reinicia a cero inmediatamente.

### 2. State Dwell (`dwell_min_ms`)

Este temporizador define un **tiempo mínimo de estancia** en un estado. Impide que el sistema abandone el estado actual, incluso si los resguardos de una transición de salida ya se cumplen, hasta que haya transcurrido el tiempo especificado.

- **Configuración:** Se utiliza el campo `dwell_min_ms` en la definición del estado o de la transición.
- **Propósito:** Es útil para estados que representan eventos clínicos que deben durar un mínimo de tiempo para ser válidos o registrados correctamente.

### Aspectos Técnicos Relevantes

- **Fase de Temporizadores:** Los contadores de permanencia se avanzan en la fase **TIMERS** del "PLC superloop", la cual es la primera fase de cada ciclo y tarda menos de 1µs en ejecutarse.
- **Observabilidad:** El progreso de estos temporizadores se puede monitorear en tiempo real. En los logs forenses (JSONL), el evento `face_dwell` incluye un campo llamado **`active_timers`**, que muestra el progreso real de las transiciones candidatas antes de que cumplan su tiempo de permanencia.
- **Diferencia con Zonas:** No debe confundirse con el `hysteresis_ms` de las zonas espaciales (`zones.toml`), el cual es un temporizador específico del `ZoneEngine` para determinar cuándo una zona se considera "vacante".

---





