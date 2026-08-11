# Manual de Configuración: Política Clínica y Lógica de Control en mana-lite

Este manual establece las directrices técnicas para la implementación de la lógica de presencia y ocupación en el ecosistema _mana-lite_. Como arquitectos de sistemas críticos, nuestro objetivo es transformar la telemetría visual en estados semánticos estables que garanticen la seguridad del paciente y la auditabilidad clínica.

## 1. Arquitectura de Control y su Relevancia Clínica

La arquitectura de _mana-lite_ se rige por una separación estricta entre **Percepción** (dominio de visión variable) y **Control** (dominio lógico determinístico). Mientras que la percepción depende de la carga computacional de la GPU y la llegada de keyframes RTSP, el sistema de control opera bajo una cadencia fija de **200ms (5Hz)** definida en la `ScanConfigSection`.

Esta desincronización es fundamental para la seguridad clínica. Al desacoplar ambos ciclos, el sistema permanece "responsivo" y predecible; incluso si la inferencia YOLO experimenta picos de latencia o caídas de frames, el ciclo de control continúa evaluando estados. Para garantizar la integridad de los datos en procesos de auditoría clínica o replays de eventos, el sistema emplea una **ScanTimeline** y un **ControlStamp**. Estos componentes actúan como anclas temporales determinísticas, vinculando cada decisión de control con el ID de frame exacto y la antigüedad de la observación, eliminando cualquier ambigüedad derivada de los relojes del sistema.

**Flujo de Datos (Bridge de Percepción a Control):** `Ingesta (RTSP) -> Percepción (Inferencia YOLO) -> Consolidación (IoU Fusion) -> Control (Presence/Tracking) -> FSM`

## 2. Configuración de la PresencePoiPolicy (Puntos de Interés)

La `PresencePoiPolicy` es el "Gatekeeper" o guardián de nuestra cadena de decisión. Su función es filtrar el ruido visual y las detecciones efímeras para confirmar un "Person of Interest" (POI). Esta política es el requisito previo para que una detección sea procesada por el filtro de Kalman.

Analizamos el impacto de los umbrales temporales en el entorno hospitalario:

|   |   |   |
|---|---|---|
|Campo|Definición Técnica|Impacto en el Comportamiento|
|`on_ms`|Tiempo acumulado de detección válida para confirmar un POI.|**Reducción de falsos positivos:** Evita que "fantasmas" de detección o personal de paso activen alarmas de ocupación.|
|`off_ms`|Tiempo de retención de la señal tras perder el contacto visual.|**Persistencia ante oclusiones:** Activa el mecanismo de **Ghosting**, permitiendo que el sistema mantenga la identidad del paciente si un clínico bloquea la cámara o si el paciente se cubre con sábanas.|

El parámetro `off_ms` es crítico: permite que el sistema extrapole la posición mediante el filtro de Kalman durante oclusiones temporales, evitando que el paciente "desaparezca" del monitor clínico ante un obstáculo transitorio.

## 3. Lógica de OccupancyPolicy y Cardinalidad de Sala

La `OccupancyPolicy` gestiona la cardinalidad de la sala (Empty, Single, Multiple). Para evitar transiciones prematuras, el `PresenceFilter` maneja estados "Ambiguos" donde la identidad no es clara, manteniendo la estabilidad clínica.

La clave de este módulo es la lógica de `**build_evidence**`. Esta función realiza una **fusión de tipo Bayesiano** entre el `PresenceFilter` (basado en clases de objeto) y el `Tracker` (basado en identidades persistentes). Esto permite que la habitación mantenga el estado "Single" incluso si el tracker pierde momentáneamente el bounding box, siempre que el filtro de presencia mantenga el "hold" sobre el sujeto.

**Estados de la OccupancyStateMachine:**

1. **Empty (Vacío):** Cero personas detectadas y expiración total de los temporizadores de salida (`empty_confirm_ms`).
2. **Single (Ocupación Simple):** Exactamente un track confirmado o una retención activa del `PresenceFilter`.
3. **Multiple (Ocupación Múltiple):** Requiere la confirmación persistente de dos o más personas durante el tiempo estipulado en `multiple_confirm_ms`.

## 4. Histéresis y Confirmación: single_confirm_ms y empty_confirm_ms

Para combatir el "flickering" o parpadeo del detector en condiciones de baja luminosidad, utilizamos primitivas de **Debouncer** y **Dwell**. El parámetro `single_confirm_ms` actúa como un **rising-edge debouncer** (desactivación por flanco ascendente); la señal de ocupación debe ser estable y continua antes de ser aceptada como una verdad clínica.

Configurar valores demasiado bajos en cuidados críticos genera "chatter" (oscilación rápida de estados), lo que satura los logs de eventos y resta credibilidad al sistema de monitoreo.

En entornos clínicos de alta fidelidad, se debe configurar `require_confirmed_tracks = true`. Esto actúa como un filtro de seguridad que solo permite que las detecciones validadas por el modelo de movimiento **Kalman7** afecten la lógica de ocupación. Al ignorar tracks "tentativos" (aquellos con pocos `hits`), eliminamos falsas alarmas provocadas por artefactos visuales que no siguen una cinemática humana coherente.

## 5. Implementación en mana.toml y Uso de Blueprints

La configuración es jerárquica, partiendo del `AppConfig` global. Los **Blueprints** permiten especializar la lógica: `detect-room-raw` se utiliza para calibrar los tiempos de la `OccupancyPolicy` observando el rendimiento bruto del detector, mientras que `detect-room-face` se emplea para el monitoreo de producción con estabilización por Kalman.

**⚠️ ADVERTENCIA DE ARQUITECTURA:** Nunca copie entradas completas de `models.toml` dentro de un blueprint. Los blueprints deben referenciar **claves estables** del catálogo global y utilizar parches (`model_overlay`) solo para campos específicos (como `confidence`), manteniendo así la integridad estructural del sistema.

**Ejemplo de configuración optimizada (TOML):**

```toml
# Configuración de Puntos de Interés (Gatekeeper del Tracker)
[presence.poi]
on_ms = 500         # Estabilidad inicial para confirmar POI
off_ms = 3000       # Margen para Ghosting ante oclusiones

# Configuración de Ocupación (Lógica de Sala)
[presence.occupancy]
single_confirm_ms = 1000       # Debouncer de entrada
empty_confirm_ms = 5000        # Tiempo de gracia para salida
multiple_confirm_ms = 1500     # Confirmación de segunda persona
require_confirmed_tracks = true # Exigir validación Kalman7
```

Para aplicar estos cambios, el sistema debe pasar por el proceso de `App::bootstrap`, que valida la consistencia entre los modelos seleccionados y las políticas de presencia. La validación final se realiza monitoreando los `presence_events` en los logs JSONL, donde se registran de forma determinística todas las transiciones de estado de la sala.