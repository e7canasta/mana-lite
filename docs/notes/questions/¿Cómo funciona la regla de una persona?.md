La **regla de una persona** es un mecanismo de control dentro del sistema de **inferencia en cascada** de Mana Lite, diseñado para activar modelos secundarios (como detección de rostros, pose o segmentación) únicamente cuando se detecta exactamente a **un individuo** en la escena.

### Propósito y Funcionamiento Técnico

El objetivo principal es la **eficiencia operativa** y la **estabilidad clínica**. Evita que el sistema malgaste recursos computacionales en ramas de inferencia costosas si no hay nadie presente o si la escena se vuelve ambigua por la presencia de múltiples personas.

- **Parámetro clave:** Se implementa mediante el campo `requires_exact_count = 1` dentro de una `CascadeRule` en la configuración del blueprint.
- **Bloqueo de rama:** Si el conteo de detecciones es cero o superior a uno, la rama del modelo hijo (ej. `face-yolo`) se bloquea automáticamente y aparece como un **skip** en las métricas.
- **Escena efectiva:** El conteo se realiza sobre el área que el detector está procesando realmente, lo que incluye cualquier Región de Interés (**ROI**) configurada. "Una persona en la sala" significa, técnicamente, una persona dentro del ROI del detector primario.

### Capas de Estabilidad

Para que esta regla sea robusta en entornos de producción (24/7), no se basa únicamente en la detección aislada de un frame, sino que utiliza capas de filtrado:

1. **Tracking:** En perfiles estables, la regla exige un **track confirmado** (que haya cumplido con el mínimo de aciertos o `min_hits`) para activar los modelos hijos.
2. **Filtro de Presencia:** Este filtro sostiene la señal de la "Persona de Interés" (POI) durante breves desapariciones del detector, permitiendo que la regla se siga cumpliendo aunque haya un "parpadeo" momentáneo en la detección.

### Blueprints que la utilizan

Esta lógica es central en varios perfiles operativos del sistema:

- **`detect-face`:** Perfil ligero para entornos de una sola persona que tolera dropouts cortos.
- **`detect-room-face`:** Activa el modelo facial solo cuando el estado de ocupación de la habitación es `single`.
- **`detect-face-pose-seg`:** Exige una persona confirmada y visible para disparar las tres ramas secundarias de forma simultánea.