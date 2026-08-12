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

Corrida del 2026-08-12, `clip1`, 180 s, 34 ventanas de 5 s.

|Magnitud|06 con visor|05 sin visor|
|---|---|---|
|`dline` p95|2,1–3,0 ms|1,9–3,0 ms|
|`dline` max / missed|4,3 ms / **0**|6,4 ms / 5|
|`cycle` p95 / overruns|201 ms / **0**|202 ms / 0|
|`evid` p50 / max|867 / 1274 ms|865 / 1272 ms|
|keyframes|176 / 176|180 / 180|
|`kf_pisados`, `img_pisadas`|**0**|0|
|`viz_pisados`|**0**|—|
|transiciones FSM|24|24|
|`in_bed`|772 scans|772 scans|

**El visor no cuesta cadencia medible sobre la pila completa.** Con `jpeg` y un
keyframe por segundo, el bridge tiene un segundo entero para drenar cada muestra
y le sobra: cero muestras pisadas en 176 keyframes.

Esto contrasta con la Fase 0, donde el visor bloqueaba el lazo 41 segundos y
forzaba 147 reconexiones (`ARCHITECTURE.md` §3.1). No es que el bridge haya
mejorado: el lazo dejó de esperarlo.

### Un número que apareció y no era del sistema

Una corrida anterior del mismo escenario dio **`viz_pisados: 55`**, cinco por
ventana en 11 de 24 ventanas. Esa corrida arrancó con `cargo run` recompilando
el binario, y el compilador se comió la máquina durante el primer tercio. La
corrida limpia, con el binario ya construido, dio cero.

Queda anotado porque el síntoma es indistinguible de un bridge que no da abasto,
y la próxima vez que aparezca lo primero que hay que preguntar es **qué más
estaba corriendo en la máquina** — no cuánto pesa el payload.

### Lo que sería un hallazgo

**`kf_pisados` o `img_pisadas` distintos de cero.** Significaría que el bridge
dejó de ser un consumidor barato y empezó a competir por el CPU con percepción o
con el lazo. Es la frontera entre "el visor pierde cuadros" —aceptable, es su
naturaleza— y "el visor le cuesta evidencia al sistema", que no lo es.
