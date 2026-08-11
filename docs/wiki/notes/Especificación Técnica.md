# Especificación Técnica: Orquestación de Cascadas y Arquitectura de Blueprints en mana-lite

## 1. Fundamentos de la Arquitectura de Orquestación

Desde una perspectiva de ingeniería de software de alto rendimiento, la importancia estratégica de la arquitectura de `mana-lite` radica en su capacidad para desacoplar el núcleo determinista del motor de inferencia de las configuraciones volátiles de despliegue. Esta separación es vital para garantizar la escalabilidad; permite que el motor de percepción mantenga una baja latencia constante mientras se introducen capas de especialización para el monitoreo clínico. Al aislar las reglas de negocio en una capa de orquestación, eliminamos el riesgo de introducir regresiones en las operaciones críticas del sistema durante el ajuste fino de entornos específicos.

El concepto de **Blueprint** se consolida como la entidad central de "lógica de pegamento" (glue logic). Un Blueprint no es un simple archivo de configuración, sino un esquema de autoridad que integra tres dominios fundamentales:

- **Selección de Modelos:** Orquestación de identidades específicas del `ModelCatalog` y sus parámetros de ejecución.
- **Lógica de Máquinas de Estado (FSM):** Definición del comportamiento semántico y transiciones basadas en eventos detectados.
- **Regiones Espaciales:** Delimitación de zonas de interés y áreas semánticas para la evaluación de proximidad y comportamiento.

Esta filosofía de diseño asegura que el sistema no sea una solución rígida de visión, sino un pipeline maleable donde la estructura estática de los archivos de configuración dicta el flujo dinámico de la telemetría.

## 2. Estructura Formal del BlueprintConfig

El struct `BlueprintConfig` actúa como el esquema de autoridad definitiva para el archivo `blueprint.toml`, definiendo la topología funcional de cada despliegue. Su misión es transformar modelos atómicos en un flujo de trabajo coherente mediante la integración de metadatos y reglas de dependencia.

Basándose en el contexto del sistema, los componentes de un Blueprint se dividen en la siguiente jerarquía técnica:

- **Metadatos de Identidad:** Establecen la raíz del pipeline, destacando el rol del `primary_model`. Este actúa como el modelo "root", procesando el frame completo para generar las detecciones base sobre las cuales se construirá la jerarquía.
- **Reglas de Cascada:** Definen objetos `CascadeRule` que orquestan la ejecución condicional. Estas reglas permiten que modelos secundarios se activen solo bajo disparadores lógicos específicos.
- **Regiones Semánticas:** Definen áreas geométricas que dotan de contexto espacial a las coordenadas normalizadas, permitiendo al sistema discernir entre regiones como "Cama" o "Puerta".

|   |   |   |
|---|---|---|
|Campo de Configuración|Propósito Operativo|Tipo de Dato|
|`primary_model`|Define el detector base (Root) para el frame completo.|`ModelId`|
|`cascade_rules`|Lista de dependencias jerárquicas entre modelos.|`List<CascadeRule>`|
|`requires_tracking`|Prioriza la estabilidad del `track_id` para reducir el jitter en cultivos.|`bool`|
|`model_overlay`|Ruta al archivo de parches para sobreescribir parámetros de modelos.|`Path`|
|`semantic_regions`|Zonas geométricas para triggers de lógica de control.|`List<Region>`|

Es fundamental notar que si `requires_tracking` es verdadero, el `InferEngine` prioriza el uso de identidades estables proporcionadas por los filtros de Kalman para realizar los recortes de los modelos hijos, asegurando una entrada visual suave y libre de ruido. Esta base estática se vuelve dinámica mediante un sistema de resolución jerárquica.

## 3. Jerarquía de Sobreescritura y Especialización de Parámetros

El proceso de carga sigue una secuencia analítica de `Load → Patch → Overlay`. Este flujo permite aplicar "parches" estratégicos a los modelos (como ajustar umbrales de confianza) según las necesidades del despliegue. Un aspecto crítico para la operatividad en contenedores es la inyección del blueprint mismo mediante la variable de entorno `MANA_BLUEPRINT_FILE`.

La **Jerarquía de Prioridad de Overrides** se resuelve de la siguiente forma (mayor a menor):

1. **Blueprint Overlay:** Archivos `models.toml` específicos dentro de la carpeta del blueprint que tienen la autoridad final.
2. **Model Definition:** Campos específicos dentro de las secciones de los archivos de tarea (ej. la sección `[models.name]` en `detect.toml`).
3. **Profile:** Parámetros heredados mediante la clave `profile` (ej. perfil "face") para estandarizar comportamientos.
4. **Task Defaults:** Ajustes predeterminados dentro de un archivo de tarea (sección `[defaults]` en `detect.toml`).
5. **Global Defaults:** Configuración base del sistema definida en `base.toml`.

Para garantizar la portabilidad entre diferentes infraestructuras de hardware, el sistema utiliza el "Rebasing" de rutas mediante la variable de entorno `MANA_MODELS_HOME`. Este mecanismo permite que las rutas relativas de los pesos ONNX definidos en el `ModelCatalog` se resuelvan dinámicamente en tiempo de ejecución, facilitando la transición entre entornos de desarrollo y producción.

## 4. Dinámica de Cascadas y Mecanismos de Recorte (Cropping)

La ejecución táctica en `mana-lite` es gestionada por la interacción entre el `InferEngine` (dentro del crate `mana-perception`) y el `CascadeScheduler`. Esta arquitectura optimiza el cómputo al ejecutar modelos pesados únicamente cuando el modelo padre cumple con criterios de disparo estrictos.

Los parámetros de una `CascadeRule` funcionan como compuertas lógicas:

- `requires`: **Determinar** la dependencia obligatoria del modelo padre (`ModelId`).
- `requires_class`: **Filtrar** la ejecución a entidades específicas (ej. "person").
- `requires_exact_count`: **Condicionar** la activación a una cardinalidad exacta (ej. ejecutar solo si hay una única persona).
- `same_frame`: **Evaluar** la temporalidad; si es `false`, el sistema puede "mirar al pasado" usando tracks previos para suavizar dropouts del detector de personas y mantener la continuidad del modelo hijo.

Para maximizar la precisión de los modelos secundarios, implementamos estrategias de recorte especializadas:

- `compute_largest_class_roi`: Selecciona el área del objeto más grande, ideal para análisis de cuerpo completo.
- `compute_upper_square_roi`: Estrategia superior para análisis facial; genera un recorte de **320x320** centrado en la mitad superior del bounding box del padre para asegurar la captura del rostro.
- `extract_crop_frame`: Ejecuta el recorte físico a nivel de buffer RGB para alimentar el siguiente nodo de inferencia.

Esta sofisticación requiere que el ecosistema pase por una validación de consistencia rigurosa antes de procesar señales en vivo.

## 5. Validación de Consistencia y Protocolo de Bootstrap

La fase `validate_bootstrap` es un pilar de seguridad de tipos y robustez operativa. La validación estática es crítica para evitar fallos de referencia en tiempo de ejecución en sistemas de monitoreo 24/7. El sistema verifica que:

1. El `primary_model` definido en el Blueprint exista y esté habilitado en el `ModelCatalog`.
2. Los `ZoneId` mencionados en los `guards` de la FSM estén presentes en el `ZoneCatalog`.
3. Todos los modelos solicitados por los estados de la FSM estén debidamente habilitados y presentes en el Blueprint actual.

La secuencia de `App::bootstrap` se desglosa en 6 etapas técnicas:

1. **Catalog Loading:** Carga de diccionarios de modelos y resolución de Blueprints.
2. **Validation:** Verificación de consistencia interna y resolución de referencias cruzadas.
3. **Perception Engine Construction:** Construcción del `InferEngine`, `Tracker` y `ZoneEngine`.
4. **Control State Initialization:** Sincronización de relojes (UTC/Monotónico) e inicialización del `FsmEngine` y el `PresenceFilter` (esencial para la Clinical Policy).
5. **Observer Wiring:** Conexión del `FanoutObserver` para telemetría JSONL y visualización en Rerun.
6. **App Assembly:** Consolidación final del objeto `App` para iniciar el bucle de ejecución.

## 6. Análisis de Perfiles de Despliegue Específicos

Los perfiles en `config/blueprints/` sirven como plantillas optimizadas para escenarios clínicos reales, demostrando la versatilidad de la orquestación:

- `**detect-room-face**`**:** Centrado en alta fidelidad. Utiliza `face-yolo` sobre un recorte superior de 320x320. Su valor reside en estabilizar la ocupación de una habitación y realizar análisis facial profundo solo cuando las condiciones son ideales.
- `**detect-face-pose-seg**`**:** Implementa un enriquecimiento total (Cara, Pose, Segmentación). Utiliza un gate de `requires_min_area_ratio` de **0.01** para filtrar detecciones diminutas que degradan la relación señal-ruido del sistema y desperdician ciclos de CPU.
- `**detect-room-raw**`**:** Un perfil de diagnóstico. Desactiva los filtros de Kalman y modelos hijos para permitir una observación cruda. Se utiliza primordialmente para el tuning de temporizadores de presencia sin la interferencia de la lógica de suavizado.

En conclusión, la arquitectura de orquestación de `mana-lite` transforma la visión computacional de una tarea estática en un sistema de monitoreo dinámico y resiliente, capaz de adaptarse a las demandas críticas del entorno clínico mediante una ingeniería de software rigurosa y modular.