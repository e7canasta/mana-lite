# 10 — Cascada con tres hijos hermanos (face + pose + seg)

## Hipótesis

El blueprint `detect-face-pose-seg` completo corre como estaba declarado desde
que el taller lo dejó sin cobertura: tres hijos hermanos —`face-yolo`,
`pose-standard`, `seg-standard`— con la misma compuerta resuelta por separado,
cada uno con su recorte y su evidencia (bbox, esqueleto, máscara), y el costo
de las tres ramas es aditivo y medible.

## Qué está activo

|Capa|Estado|
|---|---|
|Ingesta RTSP + decode|activa|
|Inferencia|**activa** — `detect-fast` + `face-yolo` + `pose-standard` + `seg-standard`|
|Tracking|**activo** (la compuerta lo requiere)|
|Zonas / FSM|apagados|
|Presencia|apagada — el tracker no la necesita|
|Rerun|apagado|

Usa el blueprint de producción `detect-face-pose-seg`, el mismo que el índice
del taller marcaba como "sin cobertura".

## Por qué este peldaño importa

Cierra la familia de perfiles ligeros: es el despliegue más pesado de la
escalera de enriquecimiento. Cada rama ya fue homologada (04 face, 07 pose, 08
seg) y la convivencia de a dos en el 09; acá lo que queda es el abanico
completo sobre una sola escena, con las tres salidas —bbox, máscara y
detección de pose— en la misma corrida.

## Cómo correr

```sh
timeout 180 cargo run --release -- --config workshop/scenarios/10-detect-face-pose-seg/mana.toml
```

Igual que el 04, conviene correr contra las dos fuentes: `home2` (cámara real,
escena vacía) y `clip1` (RTSP local con una persona en cama).

## Criterios de aceptación

|Criterio|Qué se espera|
|---|---|
|Tres compuertas separadas|Líneas de `skips` independientes para `face-yolo`, `pose-standard` y `seg-standard`, todas a 0 con una persona en cuadro|
|Tres evidencias|Bbox + confianza en las tres; campo `mask` en las detecciones de `seg-standard`; detección de pose en las de `pose-standard`|
|Sin cruce de estados|Ninguna rama condiciona a otra; las tres corren o se saltean según su propia regla (idéntica en las tres)|
|Tres recortes|`roi:[...]` por hijo, todos siguiendo al track|
|Costo aditivo|`evid:` sube respecto del 09 (costo de la tercera rama)|
|Bordes|sin `kf_pisados` — cuatro modelos dentro del presupuesto del lazo|

**La comparación que importa es 10 contra 09** (costo de la rama de seg sobre
face+pose).

### Lo que sería un hallazgo

**Una rama en silencio.** Mismo criterio que el 09, con tres líneas para
revisar.

**`kf_pisados` subiendo.** Cuatro modelos es el peor caso de los perfiles
ligeros; si el hardware no da abasto, este es el escenario que lo muestra.

## Números medidos

Corridas del 2026-08-12, 180 s cada una, en release, con `YOLO26s FP16 320`
para detección, pose y segmentación, y `YOLO12s FP16 320` para face. El `evid`
p50 es la mediana de los p50 reportados por las ventanas de 5 s; `max` es el
peor maximo reportado.

|Corrida|`evid` p50/max|`dline` p95|skips por hijo|keypoints|masks|`kf_pisados`|missed|
|---|---|---|---|---:|---:|---:|
|`clip1`, persona en cama|679 / 1080 ms|1,8-2,4 ms|13 total cada uno|164|165|0|0|
|`home2`, fuente real|599 / 1044 ms|1,8-2,1 ms|137 total cada uno|38|38|0|1|

Las tres ramas cargaron y sus compuertas fueron independientes; pose emitio
keypoints y seg emitio masks. En `clip1`, las medianas/maximos fueron
`face-yolo` 39/46 ms, `pose-standard` 36/41 ms y `seg-standard` 47/56 ms. En
`home2`, las ramas abiertas estuvieron entre 35 y 51 ms. No hubo `kf_pisados` ni
`overruns`; quedó un `missed` aislado en `home2`. La regresión de cadencia y
edad de evidencia del artifact `yolo26x` a 640 desapareció con `s/320`.

`home2` no permanecio vacia durante toda esta ventana, por eso sus skips no
representan una escena vacia pura.
