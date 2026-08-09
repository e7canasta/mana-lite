El **CascadeScheduler** es el componente del motor de inferencia encargado de orquestar el orden y la lógica de ejecución de los modelos de inteligencia artificial cuando existen dependencias entre ellos. En lugar de ser un pipeline puramente secuencial, funciona como un **programador basado en un grafo de dependencias** que decide en cada frame qué modelos deben activarse según la evidencia detectada.

### Principios de Funcionamiento

El programador organiza los modelos siguiendo un **orden topológico** (_topo-order_), asegurando que los modelos "padre" se procesen siempre antes que sus "hijos".

1. **Modelos Root:** Aquellos que no tienen dependencias (`requires` ausente) se ejecutan primero en el orden en que fueron declarados.
2. **Relación Padre-Hijo:** Un modelo secundario (como `face-yolo`) solo se programa si su modelo primario (como `detect-fast`) produce una detección que cumple con reglas específicas.

### Modos de Activación (Gating)

El scheduler maneja dos políticas principales para disparar la ejecución de modelos hijos:

- **Same-frame Gating:** Si la opción `same_frame` es verdadera, el modelo hijo se ejecuta inmediatamente sobre las detecciones producidas por el padre en el **mismo ciclo de procesamiento**. Es útil para despliegues de baja latencia o calibración.
- **Track-based Gating:** Si es falsa, el scheduler requiere que exista un **track confirmado y visible** de frames anteriores para activar al hijo. Este modo es el recomendado para entornos de producción 24/7, ya que evita activaciones erróneas por "parpadeos" o falsos positivos de un solo frame.

### Reglas de Elegibilidad (`CascadeRule`)

Para que el scheduler apruebe la ejecución de un hijo, se deben satisfacer diversos criterios definidos en la configuración:

- **Clase Requerida:** La detección del padre debe coincidir con una etiqueta específica (ej. "person").
- **Conteo Exacto:** Puede configurarse para que solo corra si hay exactamente _N_ detecciones (ej. ejecutar cara solo si hay exactamente una persona).
- **Confianza Mínima:** El objeto detectado por el padre debe superar un umbral de puntuación específico.
- **Cobertura de Región:** Se puede definir una región semántica (_SemanticRegion_) que actúe como "puerta" espacial; el hijo solo se activa si el objeto del padre se encuentra dentro de ese rectángulo.

### Integración en el Ciclo de Vida

Durante el arranque del sistema (_bootstrap_), el scheduler valida que el modelo primario sea un "root" y ordena las reglas para que la cascada sea coherente. En cada ciclo del superloop, si un modelo secundario es omitido porque no hay una detección válida que lo dispare, el sistema lo registra como un **skip** en las métricas de rendimiento.