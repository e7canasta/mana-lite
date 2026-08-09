# El Viaje de un Píxel: Cómo la IA Transforma Video en Decisiones Inteligentes

Bienvenido a esta exploración técnica. Imagine por un momento que una cámara no es simplemente un lente que graba, sino un órgano sensorial conectado a una arquitectura cognitiva capaz de entender, recordar y discernir. En el corazón de esta transformación se encuentra **mana-lite**, un ecosistema de alto rendimiento diseñado para destilar el caos de los paquetes de red invisibles hasta convertirlos en "datos con sentido" para el ámbito clínico y de seguridad.

**Misión Principal del Sistema:** "Ingerir flujos de video en tiempo real para ejecutar un pipeline de visión computacional de múltiples etapas, rastreando individuos y evaluando la ocupación espacial para determinar estados complejos de una habitación mediante una lógica de toma de decisiones robusta."

A continuación, iniciaremos un recorrido por las cinco etapas críticas donde la luz se convierte en conocimiento, comenzando por el primer contacto: la captura de la realidad.

## 1. El Despertar del Frame: De la Red a la Imagen (Ingesta)

Todo comienza en la red. El sistema recibe datos mediante el protocolo **RTSP**, donde el componente `RetinaReader` actúa como la "córnea" del sistema. Su función no es solo conectar, sino filtrar el "resplandor" del tráfico innecesario para proteger al cerebro digital.

El video viaja comprimido en **H.264 Annex-B**. Para maximizar la eficiencia, el sistema busca específicamente unidades NAL de "Tipo 5", conocidas como **Keyframes** o I-frames. Estos cuadros son autónomos y contienen la imagen completa, lo que permite al sistema descartar los cuadros intermedios (P-frames) y reducir drásticamente la carga de procesamiento. Además, para evitar el fenómeno del "Thundering Herd" (donde todas las cámaras intentan reconectarse simultáneamente tras una caída y colapsan el servidor), el sistema emplea una **lógica de Backoff con Jitter**, espaciando los reintentos de forma aleatoria y progresiva.

**Pasos críticos de la ingesta:**

- **Captura y Filtrado:** El `RetinaReader` drena el buffer de red, priorizando siempre la información más fresca y descartando paquetes antiguos para que la IA nunca reaccione a eventos del pasado.
- **Deduplicación Inteligente:** Antes de decodificar, el sistema compara los keyframes; si una imagen es byte por byte idéntica a la anterior, se suprime. No se gasta energía en procesar lo que ya conocemos.
- **Decodificación y Empaquetado:** Los paquetes se transforman en píxeles **RGB24**. Aquí se realiza la "eliminación de stride", quitando los bytes de relleno de la memoria para entregar una matriz de datos limpia y lista para la IA.

_Una vez que la "córnea" ha entregado una imagen nítida, el siguiente paso es que el sistema "entienda" qué está viendo._

## 2. La Mirada del Detective: Identificando Objetos (Inferencia)

El `InferEngine` es nuestro detective privado. En lugar de observar toda la habitación con una lupa de alta resolución (lo que consumiría recursos infinitos), utiliza una **Cascada Dinámica** de modelos YOLO.

Imagine al detective: primero observa la escena general para identificar un "sospechoso" (un cuerpo humano). Una vez localizado, el sistema realiza un **crop** (recorte) dinámico y enfoca su poder de cómputo solo en esa área para buscar detalles específicos, como una cara. Este enfoque de "zoom inteligente" permite una precisión quirúrgica sin sacrificar la velocidad del sistema.

### Comparativa de Eficiencia en Inferencia

|   |   |   |
|---|---|---|
|Método|Funcionamiento|Ventaja Clínica|
|**Cuadro Completo**|Analiza la imagen total (1080p/4K) en cada ciclo.|Simple, pero lenta y costosa en hardware.|
|**Cascada Dinámica**|Detecta un "padre" (cuerpo) y genera un "hijo" (recorte de cara).|**Alta Resolución:** Permite ver detalles ínfimos en la cara usando modelos pequeños.|

_Reconocer a alguien en un instante es solo el principio; para que la inteligencia sea útil, el sistema debe hilar el tiempo y no perder de vista al sujeto._

## 3. El Hilo de Ariadna: No Perder el Rastro (Tracking)

Aquí es donde entra el `Tracker`. Su misión es mantener la identidad de un sujeto aunque este se mueva, se gire o sea ocultado momentáneamente por un objeto. Para lograr esta persistencia, utilizamos un vector de 7 dimensiones (**Kalman7**) que no solo rastrea la posición, sino también la **velocidad de la escala** (analizando si el sujeto se está acercando o alejando de la cámara).

El sistema utiliza el **Algoritmo Húngaro** para asignar detecciones a identidades existentes, resolviendo el rompecabezas de quién es quién en cada milisegundo.

**Ciclo de Vida de una Identidad:**

1. **Tentativo** \rightarrow El sistema detecta algo, pero espera confirmación para evitar falsos positivos (ruido).
2. **Confirmado** \rightarrow El sujeto es oficial y se convierte en el motor de la lógica de la habitación.
3. **Perdido** \rightarrow Si el sujeto desaparece, el sistema crea un "fantasma" basado en su última velocidad conocida, prediciendo dónde debería estar.
4. **Eliminado** \rightarrow Si el "fantasma" no se reencuentra con su dueño tras un tiempo límite (`max_age`), el rastro se borra.

_Ahora que el sistema conoce la identidad y su trayectoria, debe interpretar el espacio físico que la rodea._

## 4. El Sentido del Espacio: Zonas y Profundidad (Lógica Espacial)

Para `mana-lite`, el mundo no es plano. El `ZoneEngine` gestiona rectángulos virtuales (Zonas de Ocupación), mientras que el `DepthAnalysis` aporta la tercera dimensión.

El gran momento "eureka" aquí es el uso de **Estadísticas Robustas (Mediana y P90)**. En lugar de promediar la profundidad (donde una mosca volando o una mano moviéndose podrían alterar el dato), el sistema utiliza la **Mediana** de los píxeles de profundidad para enfocarse en la "masa principal" del objeto. Esto permite distinguir con precisión si una persona está de pie junto a la cama o realmente acostada en ella. Además, mediante la **Calibración Métrica**, el sistema traduce las unidades del modelo de IA a metros reales, permitiendo reglas de seguridad basadas en distancias físicas.

**Ejemplo de Lógica de Profundidad:**

```text
REGLA: Detección de Paciente en Cama
SI (Track está en Zona "Cama")
Y (Mediana de Profundidad < Umbral_Calibrado_Metros)
ENTONCES -> Estado: "Persona Acostada"
EVITAR: Ignorar parpadeos visuales mediante Hysteresis de 500ms.
```

_Con la identidad y el volumen confirmados, la información fluye hacia el "director de orquesta" del sistema._

## 5. El Cerebro que Decide: La Máquina de Estados (FSM)

El `FsmEngine` es el componente que toma las decisiones finales. Utiliza "Guards" (guardias lógicas) que vigilan que se cumplan múltiples condiciones antes de cambiar el estado de la habitación.

Un concepto vital es el **Face Latching** (Anclaje de Cara). Funciona como un ancla lógica: si el sistema detectó una cara en una zona crítica, "recuerda" esa identidad incluso si la persona se gira y la cara deja de ser visible. Esto evita que el sistema dispare una alerta de "salida" errónea solo porque el sujeto dejó de mirar a la cámara. Asimismo, el sistema cuenta con **Wildcard Transitions** (transiciones `*`), que permiten saltar inmediatamente a un estado de "error" o "ciego" desde cualquier punto si el `HealthMonitor` detecta que la señal de video se ha perdido.

**Situación** **\rightarrow** **Respuesta Lógica:**

- **Habitación Vacía** \rightarrow Estado: `Idle` (En espera).
- **Persona en Cama + Cara Anclada** \rightarrow Estado: `InBed` (Monitoreo Clínico).
- **Pérdida de Señal (Wildcard** `*****`**)** \rightarrow Estado: `Blind` (Alerta de Sistema).
- **Persona Cruza Borde de Salida** \rightarrow Estado: `Exiting` (Notificación de Seguridad).

## Epílogo: El Resultado Inteligente (Observabilidad)

Todo este proceso, que ocurre en apenas milisegundos, se traduce en dos salidas fundamentales para el ser humano: los registros **JSONL** (un diario detallado de cada decisión) y la visualización en **Rerun.io**.

En Rerun.io, el sistema utiliza **Timelines duales** (conteo de cuadros y nanosegundos de captura). Esto permite a los desarrolladores y clínicos "retroceder en el tiempo" y ver exactamente qué píxeles y qué valores de profundidad activaron una alerta, garantizando una transparencia total en la toma de decisiones.

### Los 3 Grandes Beneficios de esta Arquitectura

- **Baja Latencia:** Procesamiento selectivo de I-frames y recortes dinámicos para una respuesta instantánea.
- **Robustez Clínica:** El uso de estadísticas de mediana y calibración métrica elimina las falsas alarmas por ruido visual.
- **Eficiencia Modular:** Una arquitectura de "dependencia invertida" que permite escalar el sistema desde un dispositivo local hasta un servidor de gran escala sin perder precisión.

Usted ha completado el viaje del píxel. Lo que comenzó como un paquete de datos anónimo es ahora **conocimiento clínico accionable** para salvar vidas.