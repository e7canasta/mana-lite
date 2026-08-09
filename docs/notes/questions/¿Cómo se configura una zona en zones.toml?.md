La configuración de una zona en el archivo **`config/zones.toml`** permite definir áreas rectangulares de interés (ROI) dentro del campo de visión de la cámara para monitorear la ocupación de personas y activar la lógica de la Máquina de Estados (FSM),.

Para configurar una zona correctamente, debes seguir estos parámetros técnicos:

### 1. Estructura de Definición (`ZoneEntry`)

Cada zona se define como una entrada dentro del mapa `[zones]` en el archivo TOML y requiere los siguientes campos,:

- **Coordenadas (`x1, y1, x2, y2`):** Definen el rectángulo en el espacio de coordenadas de la **imagen original** (en píxeles, antes de cualquier recorte de modelo),.
    - `(x1, y1)` representan la esquina superior izquierda.
    - `(x2, y2)` representan la esquina inferior derecha.
- **`hysteresis_ms`:** Define el retraso temporal requerido antes de que una zona se considere "vacante" después de que los tracks confirmados dejen de intersecar con ella,. Esto evita fluctuaciones rápidas en el estado de ocupación.
- **`label` (Opcional):** Un nombre legible por humanos utilizado principalmente para el registro de eventos y visualización.

### 2. Ejemplo de Configuración

Un archivo `zones.toml` típico suele tener una estructura similar a la siguiente, basándose en las entidades descritas en los fuentes,:

```
[zones]
# Definición de una zona de cama
bed = { x1 = 500, y1 = 300, x2 = 1200, y2 = 800, hysteresis_ms = 2000, label = "Cama Principal" }

# Definición de una zona de puerta
door = { x1 = 0, y1 = 100, x2 = 200, y2 = 600, hysteresis_ms = 500, label = "Entrada" }
```

### 3. Zonas Especializadas: `face_dwell`

Además de las zonas de ocupación general, el sistema admite una sección dedicada llamada **`[face_dwell]`**,.

- Esta zona es utilizada específicamente por la FSM de **Face Dwell** (en blueprints como `detect-room-face`) para activar búsquedas de rostros de alta confianza o lógicas de "persona en cama",.
- Funciona de manera independiente a los recortes dinámicos de inferencia y se publica bajo la ruta de Rerun `/world/camera/rois/fixed/face_dwell`,.

### 4. Interacción con el Sistema

- **Validación de Tracks:** El `ZoneEngine` solo considera tracks marcados como **`is_confirmed`** para determinar si una zona está ocupada.
- **FSM Guards:** Una vez configuradas, estas zonas alimentan los resguardos de la FSM como `zone_occupied`, `zone_vacated` y `all_zones_vacant`,.
- **Eventos:** Cualquier cambio de estado (ocupado/vacante) genera automáticamente un evento de tipo `ZoneEvent` que se registra en el log JSONL y se visualiza en Rerun,.