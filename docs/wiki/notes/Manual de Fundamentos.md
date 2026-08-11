# Manual de Fundamentos: El Corazón de mana-lite

Bienvenido al ecosistema de **mana-lite**. Como ingenieros y arquitectos de sistemas de IA, nuestro desafío constante no es solo hacer que una máquina "vea", sino que comprenda y actúe con la fiabilidad de un profesional clínico. Este manual te guiará a través de una arquitectura diseñada para ser robusta, predecible y, sobre todo, elegante en su ejecución.

## 1. Introducción al Ecosistema mana-lite

**mana-lite** es un pipeline de visión computacional de alto rendimiento diseñado para el monitoreo clínico y espacial. Su propósito fundamental es transformar flujos de video crudos (RTSP) en **estados semánticos accionables**, permitiendo que el sistema entienda, por ejemplo, si un paciente está en riesgo o si una habitación ha quedado vacía.

Como si de piezas de **LEGO** se tratara, su diseño modular permite intercambiar componentes para aprender nuevas tareas sin reconstruir los cimientos. Esta flexibilidad nos otorga tres beneficios clave:

- **Rendimiento de Ultra-Baja Latencia:** Optimización quirúrgica para procesar video y ejecutar redes neuronales minimizando el retraso entre el evento y la detección.
- **Lógica Determinista Inquebrantable:** El uso de máquinas de estado asegura que el sistema sea **predecible y consistente**, eliminando la volatilidad típica de las soluciones basadas exclusivamente en IA.
- **Arquitectura Modular y Extensible:** Un diseño basado en _crates_ especializados que facilita el mantenimiento y la escalabilidad del sistema a largo plazo.

Ahora que entendemos la filosofía del diseño, abramos los ojos del sistema para ver cómo los píxeles se transforman en significado.

## 2. La Dualidad del Sistema: Percepción vs. Control

En _mana-lite_, dividimos el mundo en dos dominios primarios. Imagina esta separación como la relación entre nuestros sentidos y nuestra capacidad de raciocinio.

|   |   |
|---|---|
|Dominio|Responsabilidad Principal|
|**Percepción**|El "Músculo Sensorial": Decodificación de video y ejecución intensiva de redes neuronales (YOLO).|
|**Control**|El "Cerebro Decisor": Gestión de la estabilidad temporal, lógica de negocio y toma de decisiones.|

### El equilibrio entre el caos y el orden

Las redes neuronales de la **Percepción** pueden ser ruidosas; a veces ven "fantasmas" por un solo frame. Por ello, el sistema de **Control** actúa como un filtro de calma. Mientras la percepción corre a velocidades variables, el control impone una lógica determinista que valida los datos antes de actuar, balanceando la latencia con la necesidad de una respuesta estable.

## 3. El Dominio de Percepción: De Video a Datos Semánticos

El pipeline de percepción es una línea de ensamblaje que procesa cada cuadro de video en tres etapas críticas:

1. **Ingest:** Conecta con la cámara y captura el cuadro más reciente, descartando frames intermedios para evitar el _lag_ acumulado.
2. **Infer Engine:** El motor donde residen los modelos.
3. **Detection Consolidator:** El integrador que fusiona múltiples hallazgos en una sola verdad coherente.

### El Motor de Inferencia (Infer Engine) y Cascadas

Los modelos se ejecutan siguiendo una jerarquía inteligente:

1. **Modelos Raíz (Root):** Analizan el cuadro completo (ej. buscar una "persona").
2. **Modelos Hijos (Child/Cascades):** Se activan solo si el raíz tiene éxito. Por ejemplo, si detectamos una persona, generamos un recorte dinámico (_crop_) para ejecutar un detector facial preciso solo en esa área.

### Especialización de Tareas (ModelTask)

|   |   |
|---|---|
|Tarea|Aporte a la "Visión" del Sistema|
|**Detect**|Localiza objetos y define sus fronteras con cajas (bounding boxes).|
|**Pose**|Mapea esqueletos humanos para entender posturas y movimientos.|
|**Segment**|Define la silueta exacta (píxel por píxel) para mayor precisión espacial.|
|**Depth**|Aporta la tercera dimensión, calculando distancias físicas en metros.|

### El Arte de la Consolidación: La Fusión de Datos

El **Detection Consolidator** no solo suma detecciones; realiza una "fusión semántica". Por ejemplo, mediante la lógica de **Face-to-Person Attachment**, el sistema verifica que un rostro pertenezca a un cuerpo basándose en restricciones espaciales: el rostro debe estar en la porción superior del cuerpo (`face_max_center_y_ratio`) y tener una cobertura de área lógica (`face_component_coverage`).

## 4. El Sistema de Control (mana-control): El Cerebro Decisor

Si la percepción es el ojo, **mana-control** es el corazón. Este sistema opera bajo un **Fixed-Cadence Engine**, un "latido" constante de **5Hz (cada 200ms)**. Esta cadencia actúa como un **metrónomo** que garantiza que los temporizadores y la lógica de negocio funcionen con precisión matemática, independientemente de si la GPU procesa a 30 o a 60 FPS.

Componentes vitales del control:

- **Tracker (Seguimiento)**: Implementa filtros de Kalman (SORT) para otorgar identidades. Evita que el sistema confunda a dos personas cuando se cruzan.
- **Zonas (Zones)**: Regiones espaciales (AABB) que disparan eventos de entrada/salida con histéresis para evitar "parpadeos" en los bordes.
- **FSM (Máquina de Estados Finita)**: El motor de decisiones. Para ser robusta, toda FSM en _mana-lite_ debe definir dos roles obligatorios: **Safe** (estado de emergencia ante pérdida de datos) y **Reset** (estado inicial al recuperar la señal).

**Nota de Arquitecto:** Para evitar errores por saltos de tiempo en el servidor (NTP), el sistema utiliza el **ScanInstant** (un reloj monatónico interno), asegurando que 500ms sean siempre 500ms reales para la lógica.

## 5. Vocabulario del Dominio: El Lenguaje de mana-id

Para evitar errores de escritura en el código, utilizamos `mana-id`, un sistema de identificadores con **seguridad de tipos (type safety)**. Aquí, las palabras no son simples cadenas de texto, sino entidades validadas.

|   |   |   |
|---|---|---|
|Entidad|Símbolo en Código|Propósito Educativo|
|**Modelo**|`ModelId`|Clave única del modelo (ej. `detect-fast`).|
|**Clase**|`ClassName`|Etiqueta del objeto (ej. `persona`, `cara`).|
|**Estado**|`StateId`|Nombre del paso lógico (ej. `en_cama`).|
|**Zona**|`ZoneId`|Identificador de la región (ej. `zona_puerta`).|
|**Señal**|`SignalTag`|Indicador bajo convención `dominio.atributo` (ej. `persona.presente`).|

## 6. Blueprints y Perfiles: Configurando el Comportamiento

El **Blueprint** es el "pegamento" (_glue_) que define cómo interactúan los modelos y la lógica para un caso de uso específico. Es el plano maestro de nuestra construcción.

### Perfiles Destacados

- `**detect-room-face**`: El estándar clínico; busca personas y activa análisis facial detallado solo si hay presencia confirmada.
- `**detect-face-pose-seg**`: Perfil de alta fidelidad; requiere rastreos confirmados para evitar falsos positivos en máscaras y poses.
- `**detect-room-raw**`: **Perfil de calibración**; desactiva el seguimiento para permitirnos ajustar los temporizadores de ocupación (`single_confirm_ms`) viendo la respuesta pura de la IA.

### Jerarquía de Prioridades (Overrides)

Cuando ajustamos el sistema, las configuraciones se aplican en este orden estricto (de mayor a menor importancia):

1. **Blueprint Overlay** (Ajuste específico del despliegue).
2. **Model Definition** (Configuración en el archivo del modelo).
3. **Profile** (Ajustes del perfil elegido).
4. **Task Defaults** (Valores base por tarea, ej. `detect.toml`).
5. **Global Defaults** (Configuración base del sistema en `base.toml`).

## 7. Observabilidad: Luces, Cámara y Telemetría

Un sistema que no se puede medir no se puede mejorar. _mana-lite_ desglosa su visibilidad en dos capas:

1. **Visualización en tiempo real (Rerun):** Nuestra ventana visual para depurar en vivo cajas, esqueletos y zonas.
2. **Registro estructurado (JSONL):** El historial "negro sobre blanco" de cada evento, optimizado para ser leído por máquinas sin penalizar el rendimiento.

### Sincronización mediante ControlStamp

La pieza clave aquí es el `**ControlStamp**`. Considéralo el **DNA** del sistema: un marcador que vincula cada línea del log con el frame exacto de video y la edad de la observación. Si ves un cambio de estado en el log, el `ControlStamp` te permite viajar atrás en el tiempo y ver exactamente qué vio el sistema en ese instante.

Como ingenieros, nuestra meta es la robustez. Gracias a los indicadores de salud (**Health status**) y la capacidad de auto-recuperación, _mana-lite_ no solo observa, sino que garantiza integridad en cada decisión tomada.