# 02 — Ingesta + bridge de Rerun

## Hipótesis

Con la ingesta ya homologada en el escenario 01, el bridge hacia el viewer
sostiene una conexión estable: se conecta una vez, no vuelve a reenviar el
blueprint, y el layout del viewer permanece intacto durante toda la corrida.

## Qué agrega sobre el 01

Sólo el transporte a Rerun (`[viz] enabled = true`). Inferencia, tracking,
zonas, FSM y presencia siguen apagados: cualquier inestabilidad observada
pertenece al bridge.

## Cómo correr

Abrir el viewer en `192.168.1.20:9876` **antes** de arrancar. Cada variante es
una invocación, no la edición de un campo: dejar un `image_format` flipeado de la
corrida anterior invalida la comparación sin que nada lo avise.

```sh
./run-variant.sh a-raw-native  180   # referencia sin comprimir: 6,2 MB/frame
./run-variant.sh b-jpeg-native 180   # comprimido: ~195 KB/frame
```

El script materializa la config efectiva completa en
`workshop/runs/02-ingest-viz/<variante>/mana.toml` —- junto a su salida, como
registro de qué se corrió exactamente— y al terminar imprime las compuertas.

## Criterios de aceptación

1. **Una sola conexión** — exactamente una línea `viz: connected to ...` en toda
   la corrida. Más de una es churn de reconexión.
2. **Layout estable** — el blueprint del viewer no debe reconstruirse sola. Si
   la disposición de paneles parpadea o se resetea, el bridge está recreando el
   sink.
3. **Sin regresión de ingesta** — la comparación es **acumulada**, no por
   ventana: un keyframe visto al final de una ventana se emite en la siguiente,
   así que un `processed < seen` aislado es un straddle de borde y no una
   pérdida. Lo que delata una pérdida real es la deriva acumulada (`<= 1`) o una
   ventana muerta —- `processed == 0` con tráfico entrando.
4. **Ciclo dentro del presupuesto** — `0 overruns (budget 500ms)`. Este
   criterio recién es verificable: hasta el 2026-08-11 el contador estaba
   estructuralmente muerto (ver más abajo) y ese `0` no medía nada.

## Resultados medidos (2026-08-11, corridas de 180 s)

|Variante|Conexiones|Reconexiones RTSP|Keyframes|Overruns|Veredicto|
|---|---|---|---|---|---|
|`b-jpeg-native`|1|0|179/179|0|**verde**|
|`a-raw-native`|2|**370**|68/89 (24% perdido)|**1**|degradado|

`b-jpeg-native` es la compuerta y pasa limpio. `a-raw-native` no es compuerta:
satura el enlace a propósito y existe para medir el contraste. Es la línea base
contra la que se mide la Fase 2 del roadmap, que debe volverla inofensiva.

### La cadena causal completa

En `a-raw-native` el log muestra el mecanismo entero:

```
cycle: ... max 41081ms | 1 overruns (budget 500ms)
viz: sink backlogged (10 consecutive flush timeouts) — retrying in 1000ms
rtp errors exceeded threshold — reconnecting        (x147)
```

```
enlace de viz saturado
  └─ el flush bloquea el hilo del pipeline ............ 41 segundos
     └─ retina no se poletea, el socket RTP se llena
        └─ los errores RTP superan el umbral
           └─ 147 reconexiones RTSP
              └─ 24% de los keyframes perdidos
```

**La visualización de depuración tira la ingesta de video.** No es una analogía:
el hilo bloqueado en el sink de Rerun deja de drenar el socket RTP, y la capa de
red reacciona reconectando. Es la justificación medida de ADR-033 y ADR-035.

### El contador de overruns estaba muerto

Ese `0 overruns` junto a `max 8203ms` no era un error de redondeo. La condición
era `if processed && delta_us > budget`, y el único llamador de producción
pasaba `processed: false` de forma incondicional
(`src/app/mod.rs:199`). **`cycle_overruns` no podía ser distinto de cero nunca**,
y `[health] cycle_budget_ms` se declaraba, se imprimía y no gobernaba nada.

La exención pretendía que "el ocio no declare overrun", pero confundía *no hizo
trabajo* con *estuvo ocioso*. Un ciclo ocioso duerme hasta el tick del scan y por
construcción cae muy por debajo del presupuesto: no necesitaba exención. El único
caso que la exención tapaba era el importante —- el hilo bloqueado por algo que no
es procesamiento de keyframes, que es justo lo que produce un sink lento.

Corregido: el presupuesto mide el **periodo**, no el trabajo. Fijado por
`metrics::tests::a_stalled_cycle_without_work_still_trips_the_budget`.
**Confirmado en campo** el 2026-08-11: en la corrida de 180 s de `a-raw-native`
el contador marcó 1 overrun contra un scan de 41 s. La corrección detecta el
stall real que la condición anterior dejaba pasar.

## Verificación en el viewer

Las compuertas 1, 3 y 4 salen del log y las mide `run-variant.sh`. La 2 —- que el
layout del viewer no se reconstruya solo— requiere mirar la pantalla.

Con `pipeline.infer = false` no hay detecciones, pero **sí hay un ROI fijo**:
`detect-fast` declara `static ROI [420,0 1500,1080]` y `send_fixed_rois` lo
loguea al conectar. Ese rectángulo es el control: debe caer sobre la imagen en su
posición correcta y quedarse quieto toda la corrida. Si parpadea o se
reconstruye, el bridge está recreando el sink.

Ambas variantes conservan resolución nativa, así que el ROI —- logueado en
coordenadas de píxel— no necesita compensación en ninguna de las dos.

## Qué se está verificando en el fondo

### El significado de `viz: connected`

Antes, esa línea se emitía al construir el sink. `connect_grpc_opts` es lazy:
devuelve `Ok` sin haber contactado a nadie, así que el mensaje aparecía hubiera
o no un viewer. Ahora se emite en el primer flush que devuelve `Ok`, que es la
única evidencia de que alguien está consumiendo el stream. **Si la línea
aparece, hay viewer.**

### La distinción entre contrapresión y desconexión

`flush_with_timeout` devuelve dos errores distintos y el bridge los trataba
igual:

|Error|Significado|Tratamiento actual|
|---|---|---|
|`Timeout`|No drenó en la ventana; la conexión está viva|Se cuentan consecutivos; hacen falta 10 para soltar el sink|
|`Failed`|No hay viewer del otro lado|Corta de inmediato|

Colapsarlos hacía que un frame grande sobre un enlace lento se interpretara como
caída: se soltaba una conexión sana, se reenviaba el blueprint y se reseteaban
los caches de dedup de estado, cada dos segundos.

### El costo del enlace

Un frame 1080p RGB24 son 6.220.800 B. A un keyframe por segundo son ~50 Mbit/s
sostenidos, que es lo que volvía irreal una ventana de flush de 100 ms.

El escenario corre en `b-jpeg-native`; `a-raw-native` existe para medir el
contraste, y es la que satura el enlace a propósito:

|Variante|`image_format`|Resultado|Payload|
|---|---|---|---|
|`a-raw-native`|`raw`|1920x1080|6.220.800 B|
|`b-jpeg-native`|`jpeg`|1920x1080|~195.000 B|

JPEG es el modo preferido y el único que reduce el enlace: las cajas, ROIs y
máscaras se loguean en coordenadas de píxel nativas, y como la imagen conserva
sus dimensiones no hace falta compensar geometría en ningún lado. Reducir la
escala sería la otra palanca posible, pero desalinea los overlays a cambio de
menos beneficio, así que se descartó (Fase 0 del roadmap).
