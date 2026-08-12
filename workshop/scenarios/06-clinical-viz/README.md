# 06 — La pila clínica, con ojos

## Hipótesis

**El visor no le cuesta cadencia al lazo con la pila completa encendida, y lo
que el sistema decide se puede revisar mirándolo.**

Este es el único peldaño que no agrega una capa de procesamiento. Es el 05 con
el visor prendido: mismo blueprint, mismos catálogos, mismos umbrales. Lo que
agrega es un par de ojos.

Existe porque los cinco peldaños de abajo prueban que el sistema **sostiene su
contrato temporal**, y ninguno prueba que lo que ve sea razonable. Un `evid: p50`
de 867 ms no dice si la caja está sobre la persona o sobre la mesa de luz. Eso no
lo prueba un número.

## Qué está activo

Todo lo del 05, más el bridge.

|Capa|Estado|
|---|---|
|Ingesta, inferencia, tracking|activos|
|Zonas, FSM, presencia, ocupancia|activos|
|Reglas de profundidad|activas|
|**Rerun**|**activo** — `jpeg`, calidad 75|

**`jpeg`, no `raw`.** El [escenario 02](../02-ingest-viz/README.md) midió que raw
satura el enlace y produce `viz_pisados`. Ese peldaño ya tomó la decisión; acá se
aplica en vez de volver a discutirla.

## Cómo correr

Necesita un visor de Rerun escuchando. Por defecto apunta a la estación de
trabajo, `192.168.1.20:9876`:

```sh
./workshop/scenarios/06-clinical-viz/run-fuente.sh clip1 180
```

Para mirar en la misma máquina donde corre:

```sh
rerun --port 9876 &
RERUN_ADDR=127.0.0.1:9876 ./workshop/scenarios/06-clinical-viz/run-fuente.sh clip1 180
```

### Dos fuentes, y hay que elegir a propósito

|Fuente|Qué es|Para qué sirve|
|---|---|---|
|`clip1`|`rtsp://127.0.0.1:8554/clip1` — persona en cama, en loop|**revisar la parte clínica**|
|`home2`|la cámara de la instalación|revisar la instalación real|

La cámara suele estar vacía, y una revisión visual de la pila clínica sin nadie
en escena no muestra nada: la cascada no corre, el FSM se queda en `idle` y no
hay nada que mirar. Eso no es una falla del escenario. El clip tiene la misma
cadencia de keyframe (~1/s) y la misma resolución, así que los números de
cadencia son comparables con los del 05.

## Qué mirar en el visor

|Qué|Qué diría que está mal|
|---|---|
|Caja de `detect-fast`|sobre la persona, estable entre keyframes. Si salta o toma muebles, el problema es el detector, no el lazo|
|**Recorte de `face-yolo`**|es el brazo de vuelta del lazo cerrado: sale del track y **tiene que seguir a la persona**. Un recorte congelado con la persona moviéndose significa que la directiva no está llegando|
|ROI estático del padre|`[420,0 1500,1080]`. Si la persona entra y sale de ese rectángulo, la política de recorte está mal calibrada para esta cámara|
|Línea de tiempo de estados|`/pipeline/state/room`. Con una persona acostada tiene que dominar `in_bed`|
|Zonas|`config/zones.toml` está calibrado para la cámara de la instalación. Contra el clip **no coinciden**, y eso es esperable: las zonas son de instalación, no de escenario|

Y el JSONL sirve de contraste: este escenario **se trae su propio archivo de
observabilidad** para que los eventos clínicos —FSM, zonas, face-dwell— lleguen
a disco. Es la lección del [05](../05-clinical/README.md), donde el criterio de
aceptación pedía transiciones en el JSONL y `config/metrics.toml` las tenía
apagadas. Ver `metrics.toml` acá al lado, que declara sólo sus desviaciones.

## Criterios de aceptación

|Criterio|Qué se espera|
|---|---|
|Conexión|una sola línea `viz: connected`, sin churn de reconexión|
|**Cadencia bajo carga completa + visor**|`dline` en el piso del temporizador y `0 missed`. Es el criterio principal|
|Presupuesto|`0 overruns`|
|El visor no cuesta evidencia|`kf_pisados` e `img_pisadas` en **cero**: si percepción o control empiezan a pisar, el visor está costando evidencia y eso sí es una falla|
|El FSM gobierna|transiciones coherentes con lo que se ve en pantalla|

`viz_pisados` **no es una compuerta.** El visor recibe muestras: que se pise una
significa que el bridge no llegó a tomarla antes de la siguiente, y eso es la
degradación correcta — el lazo no espera al visor. Es un número que se registra,
no un pasa/no pasa.

## Números medidos

Corrida del 2026-08-12, `clip1`, 150 s, 29 ventanas de 5 s.

|Magnitud|06 con visor|05 sin visor|
|---|---|---|
|`dline` p95|1,2–3,4 ms|1,9–3,0 ms|
|`dline` max / missed|6,7 ms / 2|6,4 ms / 5|
|`cycle` p95 / overruns|202 ms / **0**|202 ms / 0|
|`evid` p50 / max|877 / 1285 ms|865 / 1272 ms|
|keyframes|150 / 150|180 / 180|
|`kf_pisados`, `img_pisadas`|**0, 0**|0, 0|
|transiciones FSM|20|24|
|`in_bed`|634 scans|772 scans|

**El visor no cuesta cadencia ni evidencia.** Cero `kf_pisados` y cero
`img_pisadas`: ni percepción ni el lazo perdieron nada por tenerlo prendido.

Contrasta con la Fase 0, donde el visor bloqueaba el lazo 41 segundos y forzaba
147 reconexiones (`ARCHITECTURE.md` §3.1). No es que el bridge haya mejorado: el
lazo dejó de esperarlo.

### `viz_pisados`: medido, no explicado

Esta corrida dio **127** muestras pisadas, unas 5 por ventana, repartidas parejo
de punta a punta. Una corrida anterior del mismo escenario, contra la misma
fuente y con el mismo visor, dio **0**.

No sé por qué. Las hipótesis que tuve —el compilador comiéndose la máquina
durante la corrida, el payload de la pila completa contra el del 02— no las
puedo sostener: la corrida de cero también tenía la pila completa, y la de 127
no tenía nada más corriendo en la máquina. **No está explicado, y queda escrito
así en vez de con la primera explicación que sonaba bien.**

Lo que sí se puede afirmar: no le cuesta nada al sistema. En las dos corridas
`kf_pisados` e `img_pisadas` quedaron en cero, o sea que la variabilidad vive
del lado del bridge y del visor, no del lado del lazo. Que se pise una muestra
es la degradación correcta — el visor recibe muestras, no una cola.

Para la próxima, lo que hay que aislar es **el consumidor**: la misma corrida
con el visor cerrado, y después con un visor recién abierto contra uno que ya
acumuló varias corridas. Por eso el runner ahora guarda un log por corrida con
su marca de tiempo: la corrida de cero se perdió al sobrescribirse, y sin las
dos al lado no hay comparación posible.

### Lo que sería un hallazgo

**`kf_pisados` o `img_pisadas` distintos de cero.** Significaría que el bridge
dejó de ser un consumidor barato y empezó a competir por el CPU con percepción o
con el lazo. Es la frontera entre "el visor pierde cuadros" —aceptable, es su
naturaleza— y "el visor le cuesta evidencia al sistema", que no lo es.
