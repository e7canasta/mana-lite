# 04 — Inferencia + tracking y cascada

## Hipótesis

El seguimiento temporal y la cascada con hijo funcionan, y **el lazo cerrado
gobierna**: los tracks que publica control deciden si corre el modelo hijo.

Tracking y cascada entran juntos porque no son separables: la regla del hijo
pide una cantidad exacta de tracks de persona confirmados, así que una cascada
con hijo **requiere** tracker.

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
[percepción]  la regla del blueprint: ¿exactamente 1 track de persona
              confirmado, de la clase y confianza que pide el hijo?
                 sí → corre face-yolo, recorta sobre ese track
                 no → lo saltea y lo cuenta en infer_skips
```

La condición la declara el blueprint (`requires_exact_count`, `requires_class`,
`requires_min_confidence`) y la resuelve `cascade.rs`. No hay una compuerta
equivalente en el código: hubo una y se borró, ver HANDOFF §11.

## Cómo correr

```sh
timeout 180 cargo run --release -- --config workshop/scenarios/04-infer-track/mana.toml
```

## Criterios de aceptación

|Criterio|Qué se espera|
|---|---|
|La regla gobierna|`skips` en la línea de `face-yolo` **sube** cuando no hay exactamente una persona, y baja a 0 cuando la hay. Si el modelo no corrió porque el estado del FSM no lo pidió, sale como `apagados:` y no como `skips:` — son razones distintas|
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

Corridas del 2026-08-12, 180 s cada una, con `detect-fast = YOLO26s FP16 320`
y `face-yolo = YOLO12s FP16 320`. El `evid` p50 es la mediana de los p50
reportados por las ventanas de 5 s; `max` es el peor maximo reportado.

**La cámara de la instalación estaba vacía**, y un escenario de cascada sin
persona no ejercita nada: la compuerta se cierra por la razón correcta y no se
distingue de una rota. Así que hay dos fuentes. `home2` es la cámara real y
sirve de control de cadencia; `clip1` es un RTSP local con una persona en cama,
1920×1080 y ~1 keyframe/s — la misma cadencia de keyframe, que es lo que hace
comparable el atraso. La config efectiva de cada variante queda materializada
junto a su salida, como en el 02.

|Corrida|`evid` p50/max|`dline` p95|`face-yolo` skips|`kf_pisados`|
|---|---|---|---|---|
|`clip1`, persona en cama|498 / 1098 ms|1,6–1,9 ms|12 total|0|
|`home2`, fuente real|626 / 1073 ms|1,5–2,1 ms|64 total|0|

Las dos corridas terminaron con `0 overruns`, `0 missed` y `0 kf_pisados`. En
`clip1`, `detect-fast` tuvo mediana/maximo de 36/54 ms y `face-yolo` 44/134 ms;
en `home2` fueron 32/38 ms y 39/47 ms respectivamente. La compuerta siguió
abriendo con persona y cerrando sin ella.

### Antecedente histórico

### El hallazgo que sí hubo: el lazo cerrado no gobernaba

`face-yolo` se salteó en **177 de 177** keyframes con una persona quieta en
cuadro y `detect-fast` detectándola con 0,93 de confianza en todos ellos. La
compuerta no se cerraba por la escena: no podía abrirse nunca.

La compuerta de entonces —`presence_track_count`, después borrada por estar
hardcodeada— contaba tracks confirmados en la directiva, y el JSONL mide
`confirmed_count = 0` en **894 de 894 scans**. La causa estaba en
`Tracker::age_unmatched`, que ponía `hit_streak = 0` en cada scan sin medición.
La confirmación pide una racha de scans *consecutivos*, y el lazo scanea a 5 Hz
mientras la evidencia llega a 1 Hz: cuatro de cada cinco scans reseteaban la
racha, que nunca pasaba de 1.

Era la semántica correcta cuando cada scan traía su medición. Con las dos tasas
separadas (ADR-033–035) dejó de serlo, y este escenario es el primero que
ejercita ese camino — sin tracker no había tracks, y sin hijo en la cascada no
había compuerta que los consultara.

El arreglo separa las dos cosas: `associate_observations` sólo corre cuando hay
medición nueva, y los scans sin medición usan `Tracker::age_at`, que envejece la
vida del track contra el reloj de pared sin contarlo como fallo de detección.
`misses` y `hit_streak` pasan a medirse en mediciones; `max_age_ms` y
`tentative_max_age_ms` siguen midiéndose en tiempo de pared.

Se ve en el golden: desaparecieron nueve `track_lost` que se emitían una vez por
tick, y quedó uno solo en el scan que efectivamente trajo evidencia nueva, con
`misses: 4` en vez de `misses: 12`.

### Identidad del track

En 180 s sobre el clip: **4 tracks creados y 172 actualizaciones**, contra 156
creados y 20 actualizaciones antes del arreglo. Las cuatro creaciones coinciden
con los reinicios del loop del clip, donde la persona salta de posición.
