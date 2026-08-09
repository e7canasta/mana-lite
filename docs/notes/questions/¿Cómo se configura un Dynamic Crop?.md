Un **Dynamic Crop** (también referido en la configuración como `largest_class`) es un tipo de Región de Interés (ROI) que se recalcula en cada frame basándose en el cuadro delimitador (_bbox_) de un objeto detectado por un modelo padre en la cascada. Este mecanismo permite que modelos hijos, como los de detección de rostros o pose, se enfoquen únicamente en los píxeles donde se encuentra el objetivo, reduciendo la carga de cómputo y mejorando la precisión al eliminar ruido del fondo.

Para configurar un Dynamic Crop correctamente, debes seguir estos pasos y parámetros:

### 1. Requisito: Definición en la Cascada

Para que un modelo pueda usar un crop dinámico, **debe estar configurado como un modelo hijo** en el archivo `cascade.toml` o en el blueprint correspondiente. Esto es necesario porque el sistema requiere un "padre" de donde extraer las detecciones para calcular el recorte.

### 2. Parámetros de Configuración en `models.toml`

Dentro de la entrada del modelo en el catálogo (`models.toml` o el overlay del blueprint), se debe añadir la sección `[models.nombre.crop]` con los siguientes campos clave:

- **`type = "largest_class"`**: Indica que el crop es dinámico y seguirá al objeto más grande de una clase específica.
- **`class = "person"`**: Especifica la clase del modelo padre que se debe seguir (ej. "person" para detectar una cara o pose).
- **`margin`**: Un factor de expansión (ej. `0.15` para un 15%) que se añade alrededor del _bbox_ detectado para asegurar que el objeto no quede cortado en los bordes.
- **`upper_fraction`** (Opcional): Se usa para centrar el crop en la parte superior del objeto (ej. `0.50` para enfocarse en la cabeza de una persona).
- **`square_size`** (Opcional): Define un tamaño fijo en píxeles (ej. `320`) para el recorte, centrándolo en el objetivo.

### 3. Políticas de Estabilidad y Fallback

Existen tres políticas adicionales que controlan el comportamiento del crop cuando las detecciones varían:

1. **`min_region` (Piso)**: Define un área mínima que el crop siempre debe cubrir, incluso si no hay detecciones. Es útil para mantener la cámara "atenta" a una zona fija como una cama.
2. **`max_region` (Techo)**: Limita el área máxima que el crop puede ocupar. Se utiliza frecuentemente para proteger la privacidad (ej. excluir una puerta) o limitar el gasto de CPU.
3. **`fallback`**: Determina qué hacer si el padre no detecta la clase requerida:
    - `"skip"` (por defecto): El modelo hijo no se ejecuta.
    - `"full"`: El modelo corre sobre el frame completo.

### Ejemplo Práctico: Crop Facial

En el blueprint `detect-room-face`, el modelo `face-yolo` está configurado para ejecutarse sobre un **cuadrado dinámico de 320x320** centrado en la mitad superior del _track_ de una persona. El sistema aplica automáticamente los desplazamientos (_offsets_) para que, aunque la inferencia ocurra en un recorte, las coordenadas finales publicadas en el JSONL o visualizadas en Rerun correspondan siempre al frame original.