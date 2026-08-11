# El Despertar de mana-lite: Guía del Proceso App::bootstrap

## 1. Introducción: ¿Qué es el "Bootstrap"?

En ingeniería de software, el **bootstrap** o arranque es el proceso alquímico que transforma archivos de configuración estáticos en un sistema de visión artificial vivo y funcional. Para **mana-lite**, este proceso es la base sobre la cual se construye la capacidad de interpretar el mundo físico en tiempo real.

El sistema se divide en dos dominios maestros que deben sincronizarse perfectamente durante este arranque:

|   |   |   |
|---|---|---|
|Dominio|Rol Principal|Responsabilidades Clave|
|**Percepción**|El "Ojo" del sistema|Decodificación de video H.264, ejecución de redes neuronales (YOLO) y normalización de coordenadas.|
|**Control**|El "Cerebro" del sistema|Estabilización temporal, lógica de estados (FSM) y ejecución de las reglas de negocio en un ciclo de 5Hz.|

Comprender esta división es fundamental para asimilar el flujo técnico que convierte el código en una herramienta de monitoreo clínico.

## 2. Los Cimientos: Archivos y Vocabulario Crítico

Todo inicia con el archivo `mana.toml`, la entidad raíz conocida como `AppConfig`. Este archivo no solo define la infraestructura (como las URL de las cámaras), sino que establece las **Políticas Clínicas**, tales como los tiempos de espera para confirmar la presencia de una persona en una habitación.

Para navegar este proceso, debemos dominar el vocabulario central:

- **ModelId**: La clave única que identifica un modelo de IA en el catálogo (ej. "detect-fast").
- **Class**: La etiqueta específica que el modelo intenta localizar, como **"person"** o **"face"**.
- **State**: El estado lógico dentro de la **FSM (Máquina de Estados Finita)**, por ejemplo, **"searching"** o **"in_bed"**.
- **Zone**: Identificador de una región geométrica del espacio, como **"bed"** o **"door"**.

Una vez que el sistema ha definido este lenguaje, el siguiente paso es cargar las piezas que lo componen.

## 3. Etapa 1: Carga de Catálogos y Resolución de Blueprints

En esta fase, el sistema carga el `ModelCatalog`, que contiene todos los modelos disponibles. Sin embargo, para que el sistema sea eficiente, utiliza el **BlueprintConfig** como su director de orquesta.

El **Blueprint** es el pegamento que decide qué modelos usar para un escenario específico. Una de sus funciones más potentes es el **Model Patching (o Overlays)**: la capacidad de sobrescribir parámetros globales para un caso particular. Por ejemplo, un Blueprint puede reducir el umbral de confianza de un modelo de detección de rostros específicamente para una habitación con iluminación difícil, sin afectar al resto del sistema.

Con los planos y los modelos listos, el sistema debe verificar su integridad.

## 4. Etapa 2: Validación de la Consistencia

El proceso `validate_bootstrap` es la red de seguridad del sistema. No se limita a verificar si los archivos existen; realiza una comprobación cruzada profunda. El sistema asegura que los programas de la **FSM** sean consistentes con los modelos y las zonas cargadas; si una regla de negocio menciona una zona llamada "camilla", la validación confirma que esa zona esté definida geométricamente.

**Insight de Experto:** Esta etapa es vital porque evita errores catastróficos en tiempo de ejecución. Es preferible que el sistema se niegue a arrancar por una inconsistencia lógica a que falle silenciosamente mientras monitorea a un paciente crítico.

Superada la validación, procedemos a ensamblar los motores de visión.

## 5. Etapa 3: Construcción de los Motores de Percepción

Aquí se inicializan el `InferEngine`, el `Tracker` y el `ZoneEngine`. La joya de esta etapa es la configuración de las **cascadas de modelos**, un sistema de dependencias diseñado para la eficiencia extrema:

1. **Detección Raíz:** Se busca una entidad principal (ej. una persona) en el cuadro completo.
2. **Recorte Dinámico:** Si se detecta el objetivo, el sistema genera un recorte (crop) pequeño, usualmente de 320x320 píxeles.
3. **Ahorro de Cómputo:** Los modelos hijos (como el de rostro) se ejecutan solo sobre este recorte pequeño. Esto ahorra valiosos recursos de procesamiento al no analizar toda la imagen de alta definición innecesariamente.

Con los "ojos" calibrados, pasamos a configurar el centro de mando.

## 6. Etapa 4: Inicialización del Estado de Control y Sincronización

En esta fase se construye el `ControlState`, donde residen el **FsmEngine** (el motor de decisiones) y el **PresenceFilter** (que filtra detecciones ruidosas). Aquí se define el "pulso" o latido del sistema: una cadencia fija de **5Hz (un tick cada 200ms)**, independiente de cuántos cuadros por segundo entregue la cámara.

Para que el tiempo sea absoluto, el sistema ancla dos relojes:

- **boot_wall:** La hora real (UTC) para fines de registro histórico.
- **boot_instant:** Un **reloj monotónico** que mide el tiempo transcurrido desde el arranque. A diferencia del reloj del sistema, este es inmune a saltos externos (como ajustes de red por NTP), garantizando que las mediciones de duración de eventos sean precisas.

## 7. Etapa 5: Conexión de Observadores (Wiring)

Un sistema inteligente es inútil si no puede comunicar lo que ve. Aquí se configuran los sumideros de datos (sinks) como archivos JSONL y la visualización en tiempo real vía Rerun.

El componente clave es el **FanoutObserver**. Su función es clonar y distribuir la información simultáneamente a todos los receptores. Es un mecanismo de aislamiento: asegura que si un sumidero es lento (por ejemplo, una escritura lenta en disco), no bloquee ni ralentice la fluidez del motor de visualización o la lógica de control.

## 8. Etapa 6: Ensamblaje Final de la Aplicación

El bootstrap culmina con la creación de la estructura `App` definitiva. En este momento, el **RetinaReader** (encargado de la ingesta de video) se une al **ControlState** (encargado de la lógica y decisiones).

Todas las piezas individuales —motores, relojes, observadores y configuraciones— dejan de ser componentes aislados para convertirse en un único organismo funcional listo para la acción.

## 9. Conclusión: El Salto a la Ejecución (App::run)

Tras finalizar el bootstrap, el control se entrega al bucle `App::run`. Aquí, el sistema entra en su ciclo de vida activo, donde la teoría de la configuración se convierte en la práctica del monitoreo continuo.

```text
CICLO DE VIDA ACTIVO:
1. Ingesta (RetinaReader): Captura y decodifica el video.
2. Inferencia (InferEngine): Ejecuta la cascada de modelos sobre el video.
3. Control (scan loop/FSM): Evalúa estados y reglas cada 200ms.
```

Entender el bootstrap es poseer la llave de **mana-lite**. Es en este proceso inicial donde definimos no solo qué ve el sistema, sino cómo debe razonar ante lo que observa.