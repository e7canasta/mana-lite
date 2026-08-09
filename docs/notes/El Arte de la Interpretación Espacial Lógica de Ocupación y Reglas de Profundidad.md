# El Arte de la Interpretación Espacial: Lógica de Ocupación y Reglas de Profundidad


![[Pasted image 20260809030125.png]]

## 1. Introducción: De Píxeles a Entendimiento Humano

El sistema _mana-lite_ no es simplemente una herramienta de observación; es un intérprete que trasciende el flujo caótico de los píxeles para construir una narrativa coherente del comportamiento humano. En el núcleo de su arquitectura, el pipeline de visión artificial —que integra detección, seguimiento y análisis de profundidad— actúa como un traductor que convierte datos visuales crudos en "eventos semánticos". El propósito no es solo "ver" si una persona está presente, sino comprender la intención: discernir si alguien está entrando a una habitación, descansando en una zona clínica o simplemente cruzando el umbral de una puerta.

_"La geometría es el lienzo donde la máquina dibuja el mundo; la lógica es el aliento que convierte ese trazo en entendimiento."_

Para alcanzar esta sofisticación, el sistema requiere una infraestructura matemática que estandarice el espacio físico, permitiendo que la inteligencia artificial opere con una precisión quirúrgica independientemente del hardware o la resolución de origen.

## 2. El Mapa Mental de la Máquina: Geometría y Coordenadas

Para garantizar la consistencia en un ecosistema diverso de sensores, _mana-lite_ utiliza **coordenadas normalizadas** en un rango de **[0.0, 1.0]**. Esta abstracción asegura que las reglas lógicas y las zonas de interés sean inmutables, ya sea que el video provenga de una cámara 1080p o una de resolución 4K.

Sin embargo, la verdadera magia reside en cómo el sistema gestiona la dualidad de los espacios. Mientras que los modelos de IA suelen trabajar en un "Crop Space" (espacio de recorte) para analizar detalles como rostros, el `InferEngine` actúa como un puente vital, transformando estas detecciones locales de vuelta al "Frame Space" (espacio global del cuadro). Esta re-proyección, que suma los desplazamientos de origen (_offsets_) al resultado de la inferencia, es lo que permite que una cara detectada en un recorte de alta resolución se ubique con exactitud matemática dentro del mapa total de la habitación.

Para representar los objetos, el sistema alterna entre dos formatos fundamentales de cajas delimitadoras (_Bounding Boxes_):

- **Formato de Esquina (xyxy):** Esencial para la inferencia inicial, el recorte de imágenes y las operaciones de recorte (_clipping_). Define los límites absolutos de un objeto mediante sus puntos superior-izquierdo e inferior-derecho.
- **Formato de Centro (cxcywh):** Fundamental para el álgebra geométrica interna y el **Filtro de Kalman**. A diferencia de los sistemas básicos, el sistema emplea un modelo **Kalman7** con un vector de estado especializado: `[cx, cy, s, r, dcx, dcy, ds]`. Al incluir la velocidad del centro (`dcx`, `dcy`), la escala (`s`) y la tasa de aspecto (`r`), la máquina puede predecir el movimiento y la deformación de un objeto incluso durante oclusiones breves.

Esta precisión geométrica se valida mediante métricas de superposición como _Intersection over Union_ (IoU) e _Intersection over Smaller_ (IoS), herramientas críticas para determinar si un objeto pertenece a una zona o si dos detecciones corresponden a la misma entidad física.

## 3. Lógica de Ocupación: El Filtro de la Verdad

Determinar la presencia humana requiere separar el ruido del sensor de la realidad física. El sistema implementa una lógica de dos etapas que diferencia la señal técnica de la ocupación real.

### 3.1 PresenceFilter (El Debounce de Señales)

El `PresenceFilter` actúa como un amortiguador de señales para el Person of Interest (POI). Su lógica se basa en el conteo de "Ticks" (cuadros de inferencia), vinculando la estabilidad de la señal directamente con la frecuencia de procesamiento de la IA.

|   |   |   |
|---|---|---|
|Estado|Condición de Activación|Función Principal|
|**Presente**|Ticks de detección superados|Estabilizar el inicio de una acción, filtrando falsos positivos.|
|**Ausente**|Ticks de falta de detección superados|Confirmar el abandono definitivo del espacio.|
|**Signal Holding**|Detección perdida momentáneamente|Prevenir parpadeos o "flickering" durante oclusiones parciales.|

### 3.2 Cardinalidad de la Habitación

Sobre el filtro de presencia, el `OccupancyStateMachine` determina la `RoomCardinality` (Vacío, Individual, Múltiple). Aquí, como especialistas, introducimos una distinción crucial: mientras la presencia se mide en **Ticks** (dependientes de la velocidad del video), la cardinalidad se rige por **Milisegundos reales** (`std::time::Instant`). Esta **histéresis temporal** asegura que el comportamiento del sistema sea idéntico en una cámara de 10 FPS que en una de 30 FPS, evitando cambios bruscos de estado mediante ventanas de confirmación configurables.

## 4. Reglas de Profundidad: Agregando la Tercera Dimensión

El sistema utiliza mapas de profundidad monoculares para dotar a la lógica de una conciencia del eje Z. Esto permite distinguir, por ejemplo, si alguien está parado frente a una cama o si realmente está acostado sobre ella.

### Análisis Estadístico y Evaluación Dinámica

Para evitar que el ruido de un solo píxel dispare una alerta, el sistema evalúa la **intersección dinámica** entre la región de la regla (zona clínica) y el área de interés (ROI) actual de la inferencia. Sobre esta intersección, se aplican métricas robustas:

1. **Mediana:** El valor central que ignora reflejos o artefactos visuales.
2. **P10 / P90 (Percentiles):** Cruciales para identificar el "frente" (proximidad máxima) o el "fondo" (límite posterior) de un objeto dentro de una zona volumétrica.

### Calibración Métrica

Para convertir las unidades abstractas del modelo en metros físicos, se emplea una fórmula de escalado lineal de un solo punto:

distancia\_m = valor\_modelo \times \left( \frac{referencia\_física}{referencia\_modelo} \right)

Sin una referencia explícita, el sistema mantiene los datos en valores relativos, asegurando que solo se afirmen distancias métricas cuando existe una calibración real en el terreno.

## 5. El Cerebro del Sistema: La Máquina de Estados (FSM)

La `FsmEngine` es el pináculo de la jerarquía, donde la geometría, la presencia y la profundidad convergen para dictar el estado operativo del sistema.

### Los "Guardias" de la Lógica (FsmGuards)

Cada transición entre estados está protegida por condiciones que actúan como centinelas:

1. **Guardias de Zona:** Basados en eventos de ocupación o vaciado de regiones específicas en el plano 2D.
2. **Guardias de Profundidad:** Basados en el cumplimiento de reglas de distancia calibrada.
3. **Guardias de Rostro (Face Latching):** Aquí introducimos el concepto de **Memoria Espacial**. El sistema utiliza un "latch" (`face_was_inside`) que funciona como un rastro digital: si se detectó un rostro de forma estable en una zona (como una cama), el sistema "recordará" esa presencia humana incluso si la persona gira la cabeza o el detector de rostros falla momentáneamente.

### Interpretación de Comportamiento y Seguridad

El sistema maneja la incertidumbre mediante transiciones **Wildcard** (`from = "*"`). Estas actúan como una "salida de emergencia" global: si el monitor de `Health` detecta que la señal es "Stale" (estancada) o "Blind" (ciega), la FSM salta inmediatamente a un estado de error o pérdida de señal, garantizando que el sistema nunca tome decisiones basadas en datos obsoletos. Todo este ADN conductual se encapsula en **Blueprints**, permitiendo que una misma infraestructura de IA se adapte a un hospital, una residencia o un área de alta seguridad simplemente cambiando su configuración lógica.

## 6. Conclusión: La Frontera entre el Dato y la Acción

La verdadera potencia de la inteligencia artificial en _mana-lite_ no reside en la complejidad de sus cálculos, sino en la **estabilidad de su interpretación**. Al integrar la geometría de alta precisión con filtros de presencia basados en cuadros y reglas de profundidad calibradas en el tiempo real, el sistema transforma una simple cámara en un observador consciente.

Gracias al uso meticuloso de la histéresis y la memoria espacial (Face Latching), logramos que la máquina no sea esclava del ruido visual. Una IA confiable es aquella que no reacciona al primer píxel que cambia, sino a la evidencia sólida y persistente del mundo físico, permitiendo que cada dato frío se convierta en una acción humana oportuna.