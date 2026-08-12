# 03 — Ingesta + inferencia

## Hipótesis

La inferencia corre **dentro** del lazo de control y lo bloquea. Este escenario
no verifica que eso esté bien: mide cuánto, con esta cámara y este modelo.

El escenario 01 es el control del experimento. La única diferencia de
configuración es `pipeline.infer`; todo lo demás —transporte, presupuesto de
ciclo, intervalo de reporte— es idéntico, así que la comparación entre los dos
aísla el costo de inferencia.

## Qué está activo

|Capa|Estado|
|---|---|
|Ingesta RTSP + decode|**activa**|
|Inferencia|**activa** (`pipeline.infer = true`, un solo modelo)|
|Tracking|apagado|
|Zonas / FSM|apagados (catálogos omitidos)|
|Presencia|apagada (`presence.enabled = false`)|
|Rerun|apagado|

## Cómo correr

```sh
cargo run --release -- --config workshop/scenarios/03-ingest-infer/mana.toml
```

180 s como mínimo. El atraso es un fenómeno por keyframe y a ~1 keyframe/s una
ventana de 5 s tiene demasiado pocas muestras para una p95 que signifique algo.

## Qué mirar

Dos líneas, y la distinción entre ellas es el punto del escenario:

```
cycle:  5.0 Hz — 25 scans in 5s | p95 200ms max 201ms min 1ms | 0 overruns (budget 500ms)
dline:  25 deadlines in 5s | late min 0.0ms p95 ...ms max ...ms | N missed (>1.0ms)
```

- `cycle:` mide el **periodo** entre scans. Se autocorrige: un scan que arranca
  tarde empuja al siguiente, que arranca de inmediato, y el promedio se
  mantiene. Por eso se ve sano aunque el lazo esté incumpliendo.
- `dline:` mide el **atraso**: cuánto después de su vencimiento arrancó cada
  scan. No se autocorrige. Es lo que un PLC llama incumplimiento.

`missed` cuenta los vencimientos por encima de la tolerancia impresa a su lado.
La tolerancia es el piso del temporizador de tokio (1 ms de granularidad en la
rueda), no un umbral de política: por debajo de eso la medición no distingue un
incumplimiento del ruido del instrumento. La distribución a su izquierda va sin
recortar, así que el piso queda a la vista.

## Criterios de aceptación

|Criterio|Qué se espera|
|---|---|
|Atraso visible|`late max` del orden de la latencia de inferencia, no 0|
|Frecuencia|Aproximadamente un incumplimiento por keyframe procesado|
|Cadencia media|`cycle:` sigue en ~200 ms — la ráfaga compensa, y eso está bien|
|Ingesta|Sin regresión respecto del escenario 01|

**Si el atraso diera cero con la inferencia prendida, lo que está mal es la
medición, no el sistema**: sabemos que la task se bloquea mientras el modelo
corre. Un `late max` de 0 significa que el instrumento no está midiendo lo que
dice medir.

El atraso también sale al JSONL, una vez por ventana y sin depender de
`metrics_event`:

```sh
grep '"event":"scan_deadline"' workshop/runs/03-ingest-infer/*.jsonl
```

## Números medidos

Corrida del 2026-08-11, 20 ventanas de 5 s, 513 vencimientos, 104 inferencias.

|Magnitud|Medido|
|---|---|
|Bloqueo del lazo (decode + infer)|**221 ms de media = 110% de un periodo**|
|Keyframes que superan un periodo entero|**47 de 51**|
|`infer` avg|194 ms (182–247)|
|`late` p50|1,1 ms — el piso del temporizador|
|`late` p95|101–154 ms, según la fase|
|`late` max|200 ms esta corrida, 260 ms en otra|
|`missed`|5 por ventana = **1 por keyframe**; 103 de 513 (20%)|
|`cycle` p95 / overruns|300–348 ms / **0**|

**El resultado central:** la inferencia no retrasa un scan, **se come más de un
periodo entero**, y lo hace en 47 de 51 keyframes. Por eso cada keyframe
garantiza un vencimiento incumplido y obliga a una recuperación en ráfaga.

### El atraso no es una constante: es función de una fase

El `late p95` no es un número estable del sistema. Vale 141–154 ms en las
ventanas 1–9 y **da un escalón a 101–114 ms en la ventana 10**, donde se queda.
La inferencia no se movió (193 ms el primer tercio, 194 ms el último), así que
el escalón no es degradación: es un cambio de **fase**.

El bloqueo dura ~221 ms y la grilla de vencimientos mide 200 ms. Cuánto atraso
produce depende de **dónde cae el keyframe dentro de la grilla**, y esa fase la
fija el GOP de la cámara — no la controlamos. Se re-ancla cuando el GOP tiene
jitter: la ventana 12 registra `gap min 963ms`, justo después del escalón.

Lo que sí es estable, y es lo que hay que citar, es la **cota**: el atraso nunca
supera el bloqueo, y el bloqueo supera el periodo. Citar un `late p95` suelto sin
la fase que lo produjo es citar una coincidencia.

### Un episodio no reproducido

Una corrida anterior de 45 s mostró el `late p95` subiendo 84 → 223 ms de forma
sostenida, con la inferencia con excursiones a 303 ms y el `cycle max` llegando a
**458 ms contra un presupuesto de 500 ms**. La corrida larga no lo reprodujo:
inferencia plana y atraso estacionario.

Queda anotado como **no reproducido**, no como explicado. Si vuelve a aparecer,
lo que hay que mirar primero es la latencia de inferencia por frame en el JSONL
(`"type":"detection"`, campo `infer_ms`), no el atraso — el atraso es el
síntoma.

## Por qué este escenario es el pivote del roadmap

Hasta acá, el argumento para sacar la inferencia del lazo es teórico: "el modelo
tarda más que un periodo de scan". Con este número el argumento pasa a estar
medido en la instalación real, y las fases 2 a 5 dejan de ser una apuesta.
