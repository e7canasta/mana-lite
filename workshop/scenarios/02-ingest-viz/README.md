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
una invocación, no la edición de un campo — dejar un `image_max_res` flipeado de
la corrida anterior invalida la comparación sin que nada lo avise:

```sh
./run-variant.sh a-raw-native      90   # referencia: 6,2 MB/frame
./run-variant.sh b-jpeg-native     90   # nativo comprimido: ~195 KB/frame
./run-variant.sh c-raw-downscaled  90   # 960x540 sin comprimir
./run-variant.sh d-jpeg-downscaled 90   # ambas palancas
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

## Resultados medidos (2026-08-11)

|Variante|Conexiones|Keyframes|Overruns|Veredicto|
|---|---|---|---|---|
|`a-raw-native` corrida 1|**2**|36/45 (**deriva 9**)|0 (contador muerto)|degradado|
|`a-raw-native` corrida 2|1|46/46|0|limpio|
|`b-jpeg-native`|1|41/42 (straddle)|0|limpio|

**Raw nativo no está roto: está al filo.** Dos corridas consecutivas de la misma
variante, sin cambios en ese camino de código, dieron resultados opuestos. Es el
comportamiento esperable de una saturación de enlace que vive cerca del umbral,
y significa que una sola corrida no alcanza para homologar esta variante:
necesita repetición o una corrida larga.

En la corrida degradada el mecanismo quedó completo en el log:

```
20:21:16 WARN viz: sink backlogged (10 consecutive flush timeouts) — retrying in 1000ms
cycle: 2.0 Hz — 26 scans in 13s | p95 201ms max 8203ms | 0 overruns (budget 500ms)
ingest: ... pframes:65, kf_dropped:8, timeouts:56
```

Un scan tardó **8,2 s** con el hilo del pipeline bloqueado en `flush_with_timeout`
drenando frames de 6,2 MB. Durante la parada los keyframes se apilaron y el
drenaje descartó 8 —- ahí están los 9 perdidos. La guarda de contrapresión
detectó la saturación y soltó el sink para acotar el backlog, que es exactamente
para lo que se diseñó.

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
**Pendiente de confirmación en campo**: la parada no se repitió en corridas
posteriores, así que todavía no se lo vio dispararse contra un stall real.

## Verificación en el viewer

Las compuertas 1, 3 y 4 salen del log y las mide `run-variant.sh`. La 2 y la
alineación de overlays requieren mirar el viewer.

Con `pipeline.infer = false` no hay detecciones, pero **sí hay un ROI fijo**:
`detect-fast` declara `static ROI [420,0 1500,1080]` y `send_fixed_rois` lo
loguea en coordenadas nativas al conectar. Ese rectángulo sobre la imagen es el
testigo de alineación, sin necesidad de encender inferencia.

- **`b-jpeg-native`**: el ROI debe caer donde corresponde. La imagen conserva
  1920x1080, así que no hay nada que compensar.
- **`c-raw-downscaled`**: la imagen pasa a 960x540 mientras el ROI sigue en
  coordenadas de resolución completa. **Si el rectángulo aparece desplazado o
  fuera de cuadro, queda demostrado que `image_max_res > 0` desalinea** y el modo
  no debe recomendarse sin compensar geometría.

Esa comparación es la pregunta abierta del escenario.

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
sostenidos, que es lo que volvía irreal una ventana de flush de 100 ms. Este escenario corre con `image_format = "jpeg"`, que baja el payload ~32× sin tocar
la resolución.

Para aislar la contribución del encoding, correr el mismo escenario variando un
solo eje. `image_max_res` acota el **lado mayor**, así que para 1920x1080 el
factor sale de 1920:

|Modo|`image_max_res`|`image_format`|Resultado|Payload|
|---|---|---|---|---|
|Referencia|`0`|`raw`|1920x1080|6.220.800 B|
|Decimado|`960`|`raw`|960x540|1.555.200 B|
|Decimado|`720`|`raw`|640x360|691.200 B|
|Comprimido|`0`|`jpeg`|1920x1080|~195.000 B|
|Ambos|`960`|`jpeg`|960x540|~50.000 B|

`jpeg` con `image_max_res = 0` es el modo preferido: las cajas, ROIs y máscaras se loguean
en coordenadas de píxel nativas, y como la imagen conserva sus dimensiones no
hace falta compensar nada. Con cualquier `image_max_res` que achique de verdad, la imagen se reduce pero los
overlays siguen en coordenadas de resolución completa — **ese modo todavía no
está verificado visualmente y puede desalinear**. Es lo primero que este
escenario debe comprobar antes de recomendarlo.
