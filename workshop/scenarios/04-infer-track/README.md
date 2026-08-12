# 04 — Inferencia + tracking y cascada

## Hipótesis

El seguimiento temporal y la cascada con hijo funcionan, y **el lazo cerrado
gobierna**: los tracks que publica control deciden si corre el modelo hijo.

Tracking y cascada entran juntos porque no son separables. La compuerta del hijo
pregunta cuántos tracks de persona hay (`presence_track_count`, en
`src/app/inference.rs`), así que una cascada con hijo **requiere** tracker.

## Qué está activo

|Capa|Estado|
|---|---|
|Ingesta RTSP + decode|activa|
|Inferencia|**activa** — `detect-fast` + `face-yolo` en cascada|
|Tracking|**activo** (la capa bajo prueba)|
|Zonas / FSM|apagados|
|Presencia|apagada — el tracker no la necesita|
|Rerun|apagado|

Usa el blueprint de producción `detect-face`, no uno propio: a esta altura lo
que hay que validar es lo que se despliega.

## Por qué este peldaño importa

Es el primero que ejercita la **realimentación** de la arquitectura. Hasta el
03, control publicaba una directiva que percepción leía pero que no gobernaba
nada: sin tracker no había tracks, y sin hijos en la cascada no había compuerta
que consultarlos. Acá el lazo se cierra de verdad.

```
[control]  tracker.current_tracks()  ──► Slot<ControlDirective>
                                              │
[percepción]  ¿hay exactamente 1 track de persona?
                 sí → corre face-yolo, recorta sobre ese track
                 no → lo saltea y lo cuenta en infer_skips
```

## Cómo correr

```sh
timeout 180 cargo run --release -- --config workshop/scenarios/04-infer-track/mana.toml
```

## Criterios de aceptación

|Criterio|Qué se espera|
|---|---|
|La compuerta gobierna|`skips` en la línea de `face-yolo` **sube** cuando no hay exactamente una persona, y baja a 0 cuando la hay|
|El recorte sigue al track|`roi:[...]` de `face-yolo` se mueve entre ventanas — un ROI congelado con la persona moviéndose significa que la directiva no está llegando|
|Identidad estable|`track_id` en los eventos `entity` no debería renumerarse con una sola persona quieta|
|Cadencia intacta|`dline:` en el piso del temporizador y `0 missed`, igual que en el 03|
|Edad de la evidencia|`evid:` **sube** respecto del 03: dos modelos cuestan más que uno, y el piso de la edad es el costo de producirla|
|Bordes|sin `kf_pisados` — percepción debe seguir dando abasto con dos modelos|

**La comparación que importa es 04 contra 03.** El único cambio de configuración
es `track` y el blueprint; todo lo demás es idéntico, así que cualquier
diferencia pertenece a esas dos cosas.

### Lo que sería un hallazgo

**`kf_pisados` subiendo.** Significaría que dos modelos ya no entran en el
intervalo de keyframe y percepción empezó a descartar. Es degradación correcta
—descartar es lo que corresponde a una muestra— pero es el primer síntoma de que
la cascada no escala en este hardware, y hay que verlo acá y no en producción.

## Números medidos

<!-- Completar con la corrida. Un "dio bien" no sirve. -->

|Corrida|`evid` p50/max|`dline` p95|`face-yolo` skips|`kf_pisados`|
|---|---|---|---|---|
| | | | | |
