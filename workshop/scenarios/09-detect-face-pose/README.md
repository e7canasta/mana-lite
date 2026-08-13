# 09 — Cascada con dos hijos hermanos (face + pose)

## Hipótesis

Dos ramas hermanas bajo el mismo detector conviven sin pisarse: `face-yolo` y
`pose-standard` declaran la misma compuerta —exactamente una persona confirmada—
y la resuelven por separado, cada una con su recorte y sus contadores. No hay
orden entre hermanos ni dependencia entre sus resultados, y el costo de la
segunda rama es aditivo sobre el 04.

## Qué está activo

|Capa|Estado|
|---|---|
|Ingesta RTSP + decode|activa|
|Inferencia|**activa** — `detect-fast` + `face-yolo` + `pose-standard` en cascada|
|Tracking|**activo** (la compuerta lo requiere)|
|Zonas / FSM|apagados|
|Presencia|apagada — el tracker no la necesita|
|Rerun|apagado|

Usa el blueprint de producción `detect-face-pose`.

## Por qué este peldaño importa

Es el primer escenario con más de un hijo. Hasta acá la cascada se comportaba
como una cadena de dos; acá pasa a ser un abanico. Lo que se valida no es cada
rama —ya homologadas en 04 y 07— sino la **convivencia**: reporte separado,
recortes independientes y presupuesto de lazo compartido.

## Cómo correr

```sh
timeout 180 cargo run --release -- --config workshop/scenarios/09-detect-face-pose/mana.toml
```

Igual que el 04, conviene correr contra las dos fuentes: `home2` (cámara real,
escena vacía) y `clip1` (RTSP local con una persona en cama).

## Criterios de aceptación

|Criterio|Qué se espera|
|---|---|
|Cada compuerta es la suya|`skips` de `face-yolo` y de `pose-standard` aparecen como líneas separadas y bajan a 0 con una persona en cuadro|
|Sin cruce de estados|El resultado de una rama no condiciona a la otra: con una persona, ambas corren; sin ella, ambas se saltean, y las razones coinciden porque la regla es idéntica|
|Dos recortes|`roi:[...]` de cada hijo se reporta por separado y ambos siguen al track|
|Costo aditivo|`evid:` **sube** respecto del 04 (y del 07): tres modelos cuestan más que dos|
|Bordes|sin `kf_pisados` — percepción sigue dando abasto con tres modelos|

**La comparación que importa es 09 contra 04** (costo de la rama de pose sobre
la base con face) **y contra 07** (costo de la rama de face sobre la base con
pose). Las dos deltas deben sumar aproximadamente lo mismo.

### Lo que sería un hallazgo

**Una rama en silencio.** Si una de las dos no aparece ni en `skips` ni en
detecciones, hay una regla mal declarada o un modelo que no cargó — el
contador `apagados:` existe justamente para distinguir esa razón de `skips:`.

**`kf_pisados` subiendo.** El primer síntoma de que el abanico no entra en el
intervalo de keyframe en este hardware.

## Números medidos

Corridas del 2026-08-12, 180 s cada una, en release, con `YOLO26s FP16 320`
para detección y pose, y `YOLO12s FP16 320` para face. El `evid` p50 es la
mediana de los p50 reportados por las ventanas de 5 s; `max` es el peor maximo
reportado.

|Corrida|`evid` p50/max|`dline` p95|`face-yolo` skips|`pose-standard` skips|`kf_pisados`|missed|
|---|---|---|---|---|---:|---:|
|`clip1`, persona en cama|592 / 993 ms|1,2-1,4 ms|12 total|12 total|0|0|
|`home2`, fuente real|594 / 1038 ms|1,6-2,9 ms|179 total|179 total|0|0|

Los dos hijos abrieron y cerraron juntos porque comparten la misma regla, pero
se reportaron en lineas y ROIs independientes. En `clip1`, `face-yolo` tuvo
mediana/maximo de 38/46 ms y `pose-standard` 36/42 ms. No hubo `kf_pisados`,
`missed` ni `overruns`; el abanico entra en el intervalo de keyframe con este
baseline. `home2` permanecio mayormente vacia, por eso sus skips no representan
una carga sostenida de las dos ramas.

`home2` no permanecio vacia durante toda esta ventana, por eso sus skips no
representan una escena vacia pura.
