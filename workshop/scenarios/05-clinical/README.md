# 05 — La pila clínica completa

## Hipótesis

**La pila completa no le cuesta cadencia al lazo.**

Zonas, FSM y presencia encendidos sobre el blueprint que efectivamente se
despliega. Si el atraso se mantiene en el piso del temporizador con todo
prendido, el aislamiento del lazo se sostiene de punta a punta y no sólo con
inferencia.

Lo que este escenario **no** prueba es que el sistema detecte o clasifique bien.
Eso lo prueban los peldaños de abajo. Acá se prueba que la máquina sostiene su
contrato temporal mientras toma decisiones clínicas.

## Qué está activo

Todo. No queda ninguna capa por agregar.

|Capa|Estado|
|---|---|
|Ingesta, inferencia, tracking|activos|
|Zonas|**activas** — `config/zones.toml`|
|FSM|**activo** — `config/blueprints/detect-room-face/fsm.toml`|
|Presencia y ocupancia|**activas**|
|Reglas de profundidad|activas — `config/depth-rules.toml`|
|Rerun|apagado — se homologa en el 02|

**Los umbrales son los de producción a propósito.** Un escenario que afloja los
tiempos clínicos para pasar no prueba nada.

## Cómo correr

```sh
timeout 180 cargo run --release -- --config workshop/scenarios/05-clinical/mana.toml
```

180 s como mínimo: los temporizadores clínicos son de segundos
(`single_confirm_ms = 3000`, `empty_confirm_ms = 8000`) y una ventana corta no
alcanza para que el FSM transicione.

## Criterios de aceptación

|Criterio|Qué se espera|
|---|---|
|**Cadencia bajo carga completa**|`dline:` en el piso del temporizador y `0 missed`. Es el criterio principal|
|Presupuesto|`0 overruns`, `cycle` p95 en ~200 ms|
|El FSM gobierna|transiciones en el JSONL (`"type":"fsm"`) coherentes con lo que pasa en la escena|
|Los catálogos compilan|arranque sin error: si `zones.toml` o el FSM tuvieran una referencia rota, el bootstrap falla|
|Bordes|`kf_pisados` e `img_pisadas` en cero|

## El número que este escenario existe para producir

```
evid:  N scans con evidencia | edad min ..ms p50 ..ms p95 ..ms max ..ms
```

**Es el primer escenario donde esa línea significa algo**, porque es el primero
donde hay un FSM decidiendo sobre esa evidencia con temporizadores corriendo.

Lo que hay que mirar es el `max` contra los umbrales de `[health]`:

```
data_stale_ms  = 10 000    cuándo el sistema se declara ciego
stale_warn_ms  =  5 000    cuándo avisa que la evidencia envejece
```

Entre el peor caso normal y la declaración de ceguera hay casi nueve segundos, y
en ese intervalo los temporizadores clínicos siguen corriendo sobre evidencia
congelada. **Esa distancia es una decisión clínica que todavía nadie tomó
mirando este número**, porque el número no existía hasta el 2026-08-12. Ver
`HANDOFF.md` §9.

### Lo que sería un hallazgo

**`evid: max` acercándose a `data_stale_ms`** con la cámara sana. Significaría
que la evidencia no se está refrescando por una razón que no es la red, y que el
umbral de ceguera está midiendo otra cosa que lo que se cree.

## Números medidos

Corridas del 2026-08-12, 180 s cada una. Dos fuentes, por la misma razón que en
el 04: la cámara de la instalación estaba vacía y un escenario clínico sin
persona no prueba la parte clínica. `clip1` es un RTSP local con una persona en
cama, a la misma cadencia de keyframe que la cámara.

|Corrida|`evid` p50/max|`dline` p95 / missed|`cycle` p95 / overruns|transiciones FSM|
|---|---|---|---|---|
|`home2`, escena vacía|751 / 1224 ms|1,4–3,2 ms / 1|202 ms / 0|0 *(sin persona)*|
|`clip1`, antes del arreglo|736 / 1661 ms|1,5–6,8 ms / 25|205 ms / 0|**2**|
|`clip1`, después del arreglo|865 / 1272 ms|1,9–3,0 ms / 5|202 ms / 0|**24**|

**La hipótesis se sostiene.** Con la pila completa —zonas, FSM, presencia,
ocupancia, reglas de profundidad y los dos modelos de la cascada corriendo— el
atraso queda en el piso del temporizador, con `0 overruns`, `0 kf_pisados`,
`0 img_pisadas`, sin reconexiones y 180 keyframes procesados de 180 vistos.

`evid: max` llegó a **1272 ms contra `data_stale_ms = 10 000`**: factor 8, con la
cámara sana. El hallazgo que este README anticipaba —la edad de la evidencia
acercándose al umbral de ceguera— no ocurrió.

### El FSM gobierna, y decide lo correcto

|Estado|scans|
|---|---|
|`in_bed`|772|
|`other`|66|
|`detected`|24|
|`searching`|10|
|`idle`|23|

24 transiciones, arrancando en `idle → searching → in_bed`. El estado dominante
para una persona acostada es `in_bed`, que es lo que corresponde. `cara.presente`
sale de la cascada en 856 de 895 scans, y el recorte dinámico se mueve: **165
recortes distintos en 170 llamadas** a `face-yolo`.

### Los dos hallazgos

**1. El escenario no podía producir su propia evidencia.** El criterio de
aceptación de arriba pide transiciones en el JSONL, y este escenario incluye
`config/metrics.toml`, que trae:

```toml
fsm_events        = false
zone_events       = false
face_dwell_events = false
```

Las primeras corridas salieron con **cero** apariciones de `idle`, `blind`, `fsm`
o `face_dwell` en el JSONL, y por un rato eso se leyó como "el FSM no
transiciona". Un test contra el catálogo real —`scan_con_una_persona_transiciona_
el_fsm_de_produccion`, en `mana-control`— pasó, y ahí se dio vuelta el
diagnóstico: el FSM transicionaba, el instrumento estaba apagado.

Es la regla 5 del HANDOFF en su peor forma: un escenario que declara un criterio
que su propia configuración vuelve inverificable. **Queda abierto** decidir si
los eventos clínicos van encendidos por defecto o si este escenario se trae su
propio archivo de métricas.

**2. La cascada nunca corría.** Con los eventos encendidos, el FSM llegaba hasta
`other` —*"Persona presente sin cara visible"*— y se quedaba ahí en 561 de 605
scans, porque `cara.presente` no puede ser `true` si el modelo hijo nunca corre.
La causa está documentada en el [escenario 04](../04-infer-track/README.md): el
tracker no confirmaba ningún track. El ciclo de vida de la cara no arrancaba.

Los números de la fila "después del arreglo" son con esa corrección aplicada y
**sin tocar ninguna otra cosa**: mismos catálogos, mismo blueprint, mismos
umbrales.
