# Plan de Configuración de Inferencia: Optimización de Despliegues Clínicos con Mana Lite

La implementación de sistemas de visión computacional en entornos de salud de misión crítica exige un equilibrio riguroso entre la potencia analítica y el determinismo operativo. En dispositivos de borde (_edge_), donde los recursos computacionales son finitos y las condiciones térmicas son extremas, la estrategia de despliegue debe priorizar la seguridad clínica sobre la concurrencia. El uso de una arquitectura de binario único, orquestada mediante un superloop de ejecución sincrónica inspirado en los Controladores Lógicos Programables (PLC), garantiza que el razonamiento del sistema sea predecible y verificable, eliminando las condiciones de carrera inherentes a los sistemas asíncronos.

## 1. Fundamentos de la Arquitectura de Ejecución Sincrónica

La decisión técnica fundamental de Mana Lite (ADR-001) es la adopción de una **Arquitectura de Binario Único (Single Binary Architecture)**. A diferencia de arquitecturas multi-proceso que dependen de comunicaciones entre procesos (IPC) complejas, este enfoque consolida toda la lógica en un solo proceso estático. Se rechazaron las arquitecturas puramente asíncronas para evitar estados inconsistentes en la evaluación de la lógica clínica, asegurando que cada decisión del sistema se base en un _snapshot_ consistente de los datos.

La ejecución se organiza a través de un **PLC Superloop** (ADR-003), que procesa cada ciclo mediante siete fases ordenadas secuencialmente:

1. **TIMERS:** Actualización de contadores y tiempos internos del sistema.
2. **EVALUATE:** Verificación de cambios en el estado de las zonas y temporizadores.
3. **INGEST:** Lectura no bloqueante del flujo de video para obtener el cuadro más reciente.
4. **INFER:** Ejecución de los modelos de inteligencia artificial (fase de mayor costo).
5. **ZONES:** Procesamiento de intersecciones espaciales tras nuevas detecciones.
6. **FSM:** Evaluación de la máquina de estados finitos según el contexto clínico detectado.
7. **PUBLISH:** Emisión atómica de eventos y métricas de salud del sistema.

Esta estructura elimina la complejidad del IPC y garantiza un tiempo de ejecución del ciclo (WCET) medible. Aunque la ejecución en el hilo principal limita el rendimiento a la suma del tiempo de decodificación e inferencia, este sacrificio en paralelismo se traduce en una robustez superior para aplicaciones de seguridad del paciente. Esta modularidad operativa se gestiona externamente mediante una configuración basada en archivos TOML.

## 2. El Sistema de Blueprints y el Catálogo de Modelos

Los **Blueprints** (ADR-026) actúan como el "perfil operativo" que orquestra la inteligencia del sistema sin alterar el binario base. Definen qué modelos ejecutar y bajo qué reglas de dependencia, permitiendo que el núcleo tecnológico se adapte a escenarios que van desde la calibración inicial hasta el monitoreo 24/7.

El sistema sigue el patrón de catálogo TOML (ADR-002), distribuyendo las responsabilidades en archivos especializados:

- `mana.toml`: Raíz de la aplicación que gestiona la fuente de video y referencias.
- `models.toml`: Manifiesto público de los modelos ONNX disponibles.
- `zones.toml`: Definición de regiones de interés (ROI) y parámetros de ocupación.
- `fsm.toml`: Reglas de política clínica, estados y transiciones.

Con la introducción de los **Blueprints de Inferencia Nombrados** (ADR-026), el sistema permite implementar _overlays_ para sobrescribir parámetros sin mutar el catálogo compartido.

|   |   |   |
|---|---|---|
|Característica|Catálogo Tradicional|Blueprints con Overlays|
|**Selección de Modelos**|Basada en el inventario global compartido.|Selección explícita de un subconjunto operativo.|
|**Modelo Primario**|Definido estáticamente por tarea.|Selección explícita del modelo raíz del grafo.|
|**Flexibilidad**|Cambios afectan a todos los despliegues.|Permite ajustes específicos por cada despliegue.|
|**Gestión de Cascadas**|Definida de forma rígida en el catálogo.|Orquestación dinámica del grafo de ejecución.|
|**Política de Activación**|Única para todo el sistema.|Alterna entre políticas (Same-frame / Tracked).|

El mecanismo de resolución compone una vista derivada del catálogo en tiempo de ejecución. Al cargar un Blueprint, el sistema habilita solo los componentes necesarios, facilitando la implementación de estrategias avanzadas de eficiencia térmica.

## 3. Estrategias de Eficiencia Térmica y Optimización de GPU

El sobrecalentamiento en dispositivos _edge_ (como la plataforma Jetson) es el principal enemigo de la disponibilidad. La **inferencia perezosa** es la solución crítica para garantizar una operación continua 24/7.

- **I-Frame y NAL-level Gating (ADR-007, ADR-004):** El sistema inspecciona los tipos de unidades **NAL tipo 5 (IDR)** en el flujo H.264 antes de la decodificación. Esto permite descartar cuadros P/B en microsegundos, reduciendo la carga de decodificación en un 97%.
- **Ghost Mode (ADR-007):** Cuando el gating está activo, las fases FSM y ZONES continúan evaluándose en cada ciclo de reloj utilizando las últimas detecciones conocidas como "ghost detections". Esto permite que los _dwell timers_ clínicos sigan avanzando y que el sistema mantenga su estado lógico incluso entre la llegada de I-frames distantes.
- **Preprocess Tensor Cache (ADR-010):** Optimización _eager_ por tamaño de imagen (`imgsz`). Si varios modelos comparten resolución (ej. 320px), el reescalado y la normalización se ejecutan una sola vez, ahorrando hasta 3ms por cada modelo adicional en la cascada.
- **Interval-Based Model Gating (ADR-016 - Roadmap):** Esta funcionalidad planificada permitirá definir intervalos mínimos (ej. 0.5 fps para identificación facial) para evitar la saturación de la GPU sin perder relevancia clínica.

Estas optimizaciones preparan el entorno para la ejecución de cascadas inteligentes de modelos.

## 4. Diseño de Cascadas Multimodelo y Scopes Semánticos

La jerarquía de modelos utiliza una relación **Padre-Hijo** (ADR-005, ADR-016) para que el análisis de alta resolución solo ocurra cuando el contexto semántico lo justifique. Un modelo hijo (ej. `face-yolo`) solo se activa si el "Detector Raíz" (ej. `detect-fast`) cumple con los filtros de `requires_class` y `requires_min_confidence`.

El concepto de **Scopes Semánticos** y recortes dinámicos es fundamental:

- **Dynamic Crop (ADR-023):** Utiliza la lógica de `extract_crop_frame` (ADR-022) para crear un área de análisis centrada en la entidad de interés (ej. cuadrado de 320px en la mitad superior del cuerpo). Este recorte puede exceder deliberadamente el ROI del padre para no perder la detección en los bordes del área estática.
- **Transformación de Coordenadas (ADR-012, ADR-024):** Dado que los modelos hijos operan en el "Crop Space", el sistema realiza un mapeo inverso automático al "Frame Space" global. La fórmula aplica el factor de escala y el padding derivados de `PreprocessedFrame` (ADR-010), sumando los desplazamientos `offset_x` y `offset_y` para reintegrar las detecciones a la escena completa.

## 5. Políticas de Activación y Tracking Avanzado

Para evitar falsos positivos en eventos críticos como una **"Alerta de Salida de Cama"**, el sistema filtra el ruido sensorial mediante políticas de activación robustas (ADR-026):

1. **Same-frame:** Ejecución inmediata sobre la detección del padre. Útil para calibración rápida.
2. **Confirmed-track:** El modelo hijo solo se activa si el padre posee una identidad temporal estable. Es la política recomendada para monitoreo clínico, ya que ignora detecciones efímeras.

La identidad temporal es gestionada por el **Tracking SORT** (ADR-013), que implementa un **Filtro de Kalman 7D** (rastreando posición, escala y velocidad) y el **Algoritmo Húngaro** para la asociación global óptima. Parámetros como `min_hits` y `max_age` (ADR-013) garantizan la estabilidad frente a oclusiones momentáneas, mientras que el "Presence Filter" realiza un _debouncing_ de señales para asegurar que solo los rastros confirmados disparen la lógica de negocio.

## 6. Lógica de Negocio Clínica y Gobernanza de Datos

El **FSM Engine** (ADR-015) es el cerebro que transforma coordenadas en alertas operacionales. Evalúa una jerarquía de prioridades que incluye _Guards_, _Dwell Timers_ ( Ton) y **Wildcard transitions** para estados de seguridad como `blind` (pérdida de señal).

La detección de comportamientos complejos se apoya en dos pilares adicionales:

- **Hysteresis con lógica Tof (ADR-014):** El motor de zonas utiliza un temporizador de retardo a la desconexión (_off-delay_) para evitar que una falla momentánea en el tracking dispare un evento de "zona vacía" erróneo.
- **Reglas Clínicas de Profundidad (ADR-025):** El sistema utiliza estadísticas robustas como **Mediana y Percentiles (P10/P90)** para filtrar _outliers_ de ruido en mapas de profundidad monocular. Esto permite validar eventos como la **"Detección de Acercamiento a Cama"** con una referencia física de distancia.

Finalmente, la gobernanza de datos se asegura mediante el formato **JSON Lines (JSONL)** (ADR-006). Cada evento (`meta`, `health`, `detection`, `fsm`) es un registro atómico y auditable, consolidando a Mana Lite como una infraestructura robusta y transparente para el despliegue de IA en entornos de salud de alta exigencia.