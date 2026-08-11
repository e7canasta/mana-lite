# Protocolo de Arquitectura: Orquestación de Percepción Multi-Etapa y Gestión de Cascadas

Este protocolo establece las directrices técnicas para el diseño y la implementación de sistemas de visión artificial de alto rendimiento dentro del ecosistema `mana-lite`. Como arquitectos, nuestra misión es garantizar que la infraestructura sea capaz de transformar flujos de video brutos en señales semánticas estables. La clave de esta robustez reside en el **Blueprint**: un motor de orquestación estratégica que permite desacoplar la infraestructura física del dominio lógico, manteniendo la integridad de un **binario único** mientras se transita entre el monitoreo ligero y la supervisión clínica de alta fidelidad.

### 1. Marco Conceptual: El Rol de los Blueprints en la Arquitectura

En la arquitectura de `mana-lite`, el Blueprint no es un simple archivo de configuración; es el nexo integrador que vincula el `ModelCatalog`, las zonas espaciales y la lógica de la FSM. Esta capa de abstracción permite que el sistema altere dinámicamente su comportamiento sin recompilar el núcleo, facilitando despliegues específicos mediante la estructura `BlueprintConfig`.

**Jerarquía de Carga y Resolución de Modelos** El sistema emplea un proceso de resolución jerárquico **Load → Patch → Overlay** que garantiza la portabilidad absoluta:

- **Load**: Se localizan los activos `.onnx` y manifiestos `.toml` base utilizando la variable de entorno `MANA_MODELS_HOME`.
- **Patch**: Se aplican configuraciones por tarea (ej. `detect.toml`, `pose.toml`) que definen comportamientos por defecto.
- **Overlay**: El Blueprint introduce un `ModelPatch` (vía `model_overlay`) que actúa como la última palabra en la cadena de mando, ajustando umbrales de confianza (`confidence`) o resoluciones (`imgsz`) para necesidades específicas del despliegue.

**Resolución de Configuración del Catálogo:**

- `MANA_MODELS_HOME` (Ancla de portabilidad)
    - → `models.toml` (Catálogo Global)
        - → `detect.toml` / `pose.toml` (Parámetros de Tarea)
            - → **Blueprint Overlay** (Ajustes de despliegue específicos)
                - → `ValidatedBootstrap` (Estado inmutable en tiempo de ejecución)

### 2. Diseño de Cascadas y Reglas de Dependencia (CascadeRules)

Las jerarquías de ejecución o **cascadas** son la base de nuestra eficiencia computacional. Al condicionar la ejecución de modelos pesados (pose, segmentación) a los resultados de modelos "raíz" más livianos (`detect-fast`), optimizamos el presupuesto de GPU y reducimos la latencia del pipeline.

**Parámetros Críticos de CascadeRule:**

- `**requires**`: Identificador único (`ModelId`) del modelo padre que actúa como disparador.
- `**requires_class**`: Filtro semántico (ej. "person") que valida la pertinencia del análisis hijo.
- `**requires_exact_count**`: Guardián de recursos; permite, por ejemplo, activar el análisis facial solo en el estado "single" (cardinalidad = 1), evitando saturar el pipeline en escenas con múltiples sujetos.

**Sincronización Temporal y Estabilidad (Kalman7)** La gestión de caídas momentáneas del detector se resuelve mediante la diferenciación entre `same_frame` y `requires_tracking`. Cuando `same_frame = false`, el sistema utiliza el modelo de movimiento **Kalman7** para predecir y extrapolar el ROI basándose en tracks previos. Esto garantiza que el análisis de alta fidelidad continúe incluso si el detector raíz falla por oclusión o ruido en un frame específico.

**Tabla de Referencia: Reglas de Cascada vs. Impacto en el Pipeline**

|   |   |
|---|---|
|Regla de Cascada|Impacto Operativo en la Percepción|
|`requires`|Establece la dependencia topológica y el orden de ejecución.|
|`requires_class`|Filtro de pertinencia semántica; previene inferencias irrelevantes.|
|`requires_exact_count`|Control de cardinalidad; protege el ciclo de GPU en escenas saturadas.|
|`same_frame = false`|Activa la predicción **Kalman7** para suavizar dropouts del detector.|

### 3. Implementación de Dynamic Cropping y Análisis de Alta Fidelidad

El **Dynamic Cropping** es nuestra estrategia para maximizar el ratio de información por píxel. Permite que modelos con resoluciones de entrada modestas operen sobre detalles extraídos del frame original, manteniendo una precisión clínica sin el costo de procesar frames completos en 4K.

**Estrategias de Cómputo de ROI (Region of Interest):**

1. `**compute_largest_class_roi**`: Seguimiento del objetivo predominante con márgenes de seguridad dinámicos.
2. `**compute_upper_square_roi**`: Específicamente diseñado para rostros; calcula una región cuadrada basada en la fracción de altura superior del cuadro delimitador del padre para asegurar la captura del área craneal.

**Mecánica de Ejecución en el InferEngine:** El proceso de extracción y traducción es crítico para la integridad espacial. El `InferEngine` sigue estrictamente esta secuencia:

1. **ROI Preparation**: Cálculo del recorte basado en la estrategia configurada.
2. **Extraction**: Uso de `extract_crop_frame` para obtener el sub-frame del buffer RGB.
3. **Prediction**: Ejecución del modelo hijo, retornando resultados brutos de `ultralytics_inference::Results`.
4. **Parsing & Translation**: Conversión de los resultados en estructuras `Detection` y traducción de coordenadas locales del recorte al sistema de coordenadas global del frame.

### 4. Consolidación de Detecciones y Salida de Percepción

Para evitar la redundancia y el ruido, el `DetectionConsolidator` unifica la visión fragmentada de múltiples modelos en un estado semántico coherente. La fusión utiliza la métrica IoU; si dos detecciones de la misma clase superan el umbral `same_class_iou`, se consolidan priorizando la geometría del `primary_model`.

**Arquitectura de Adjudicación Face-to-Person:** El sistema no trata al rostro como una entidad independiente, sino como un componente del cuerpo. Esto es vital para el seguimiento de entidades únicas:

- **Coverage**: El cuadro del rostro debe estar contenido significativamente dentro del padre.
- **Vertical Position**: Validación anatómica del centro Y (debe estar en el tercio superior).
- **Suppression**: Una vez adjudicado, el rostro se elimina como entidad independiente. Esto evita que el sistema de control genere "tracks fantasma" para una cara que ya pertenece a una persona rastreada.

El producto final es el `**ClinicalSample**`, que encapsula la validez de la señal (`signal_valid`), el conteo consolidado (`raw_person_count`) y las evidencias recopiladas para la FSM.

### 5. Optimización de Recursos y Reglas de Control Crítico

En despliegues de tiempo real, la eficiencia no es negociable. Utilizamos el `PostprocessConfig` para aplicar filtros rigurosos como `min_area_ratio` (típicamente 0.01), descartando detecciones minúsculas que solo inyectarían ruido a la FSM.

**Determinismo en la Cadencia (Scan Loop):** Existe una desconexión intencional entre la frecuencia de inferencia y el tick de control. El loop `scan()` opera a una cadencia fija de **200ms**. Este determinismo garantiza que los timers de presencia (`on_ms`, `off_ms`) sean consistentes, independientemente de si la GPU procesa a 15 o 60 FPS.

**Mejores Prácticas para la Optimización de Cascadas**

- **Gestión del Cycle Budget**: Monitorear el tiempo de ejecución del `scan()`; cualquier excedente sobre el presupuesto de ciclo es un evento de salud crítico.
- **Filtrado Pre-Inferencia**: Implementar `requires_min_area_ratio` para evitar ciclos de GPU en objetivos lejanos de baja calidad.
- **Cascadas Condicionales**: Activar modelos de alta fidelidad solo en estados específicos (ej. estado "single" en el perfil `detect-room-face`).

### 6. Validación de Arquitectura y Secuencia de Bootstrap

La robustez del sistema `mana-lite` se garantiza mediante el proceso `validate_bootstrap`. Antes de iniciar el primer frame, el sistema realiza una inspección profunda de la integridad del Blueprint.

**Checklist de Validación para el Arquitecto:**

- [ ] **Consistencia de Catálogo**: Verificar que todos los `ModelId` y `ZoneId` referenciados existan en sus respectivos catálogos.
- [ ] **Integridad de FSM**: Confirmar la existencia de los tres roles obligatorios: `initial`, `safe` (para pérdida de señal) y `reset`.
- [ ] **Análisis de Dependencias**: Validar que no existan dependencias circulares en las `CascadeRules`.
- [ ] **Validación de Clases**: Asegurar que las etiquetas de `requires_class` coincidan con la salida real del modelo padre.

Mediante este rigor arquitectónico, transformamos una simple tubería de video en un sistema de percepción clínica capaz de entregar señales estables, deterministas y accionables bajo cualquier carga operativa.