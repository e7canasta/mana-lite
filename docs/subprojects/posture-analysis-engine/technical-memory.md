# Memoria tecnica: Engine de Analisis de Postura por Firmas

**Estado:** diseno inicial para comenzar construccion
**Fecha:** 2026-08-13
**Modelo de referencia:** `depth-l-640`
**Fuente experimental:** siete muestras de `samples` y cuatro frames vacios

## 1. Alcance de esta memoria

Las memorias anteriores de `deep-fusion` describen hipotesis y componentes
aislados. Esta memoria fija el marco para el engine nuevo. No todas las
conclusiones exploratorias se consideran contrato.

El engine debe trabajar sobre los componentes ya disponibles en mana-lite:

- `SurfaceCalibration` y `deep-calib` para superficies de la escena.
- `BodyPartsEstimator` para geometria derivada por partes.
- pose, face y segmentacion como fuentes independientes.
- `depth-scene` para referencias de cama y piso.
- `deep-calib-radio` y `deep-calib-parts` como artefactos diagnosticos.

El engine nuevo no ejecuta modelos en su primer corte. Consume resultados
producidos por esas piezas, los compara contra perfiles TOML y explica el
resultado en JSON.

## 2. Evidencia actual

La sesion l-640 fue recalibrada con cuatro frames vacios. La ROI es:

```text
[452, 140, 1300, 1029]
```

Las referencias de la escena quedan aproximadamente asi:

```text
bed/head    1.329
bed/body    1.097
bed/feet    0.944
floor/head  1.773
floor/body  1.415
floor/feet  1.160
```

La variacion entre frames vacios fue de hasta `0.021` en una zona y todas las
zonas tuvieron `valid_ratio=1.0`. Esto valida la calibracion como baseline
relativo de esta camara y modelo, no como medicion fisica universal.

La matriz descriptiva actual contiene estas observaciones:

| muestra | bbox aspect | seg area | torso tilt | head-torso | legs-torso | depth spread | min mask |
|---|---:|---:|---:|---:|---:|---:|---:|
| `acostado-1` | 0.646 | 0.215 | 14.6 | +0.138 | -0.310 | 0.494 | 0.600 |
| `sentado-1` | 0.544 | 0.135 | 4.1 | +0.046 | -0.107 | 0.164 | 1.000 |
| `sentado-borde-1` | 0.526 | 0.132 | 27.2 | +0.100 | +0.098 | 0.132 | 0.667 |
| `parado-aside-1` | 0.580 | 0.184 | 26.3 | +0.008 | -0.006 | 0.045 | 0.600 |
| `leaving-bed-aside-head-1` | 0.535 | 0.122 | 28.0 | +0.211 | -0.142 | 0.366 | 0.600 |
| `foot-left-bed-2` | 1.164 | 0.045 | 3.4 | +0.144 | -0.150 | 0.311 | 0.000 |
| `foots-left-bed-1` | 1.192 | 0.119 | 4.5 | +0.143 | -0.163 | 0.346 | 0.000 |

Estos numeros son semillas de firmas, no umbrales finales. Cada fila proviene
de una sola muestra corporal y debe tolerar variacion de persona, ropa,
occlusion y distancia dentro de la escena.

## 3. Marco de pensamiento adoptado

La postura no es una clasificacion de depth. Es un consenso de evidencias:

```text
geometria 2D + partes con mascara + profundidad relativa + persistencia
```

### 3.1 Geometria 2D

Es la estructura primaria porque sigue siendo util cuando depth deriva o una
zona calibrada queda parcialmente ocluida. Incluye:

- bbox de persona: ancho, alto, aspect y area relativa.
- bbox de face cuando existe.
- posiciones normalizadas de keypoints.
- inclinacion del torso.
- extension y angulos de brazos y piernas.
- area, bbox y componentes de la mascara.

### 3.2 Partes corporales

`BodyPartsEstimator` es la representacion corporal principal. La parte puede ser
completa, parcial o ausente:

- `head`: face como ancla, joints de cabeza y mascara.
- `torso`: hombros y caderas.
- brazos: hombro, codo y muneca por lado.
- piernas: cadera, rodilla y tobillo por lado.

Un lado ausente no invalida el lado visible ni todo el actor. La geometria se
conserva junto con `support`, `quality` y `mask_coverage`.

### 3.3 Profundidad

`Metric` significa que se usa directamente el valor de profundidad producido
por el modelo para colorear y comparar dentro de la misma camara/modelo/ROI. No
significa que el valor sea una distancia fisica en metros.

La profundidad aporta:

- `median`, `p10`, `p90` y cobertura por parte.
- diferencia relativa al torso.
- relacion con la zona de cama/piso cuando la interseccion geometrica existe.

No debe decidir una postura por si sola. En particular, una cara o un keypoint
aislado puede caer en una zona equivocada por solapamiento de profundidad.

### 3.4 Consenso

Cada componente produce un score continuo y razones. El resultado solo se
clasifica cuando hay suficientes componentes observados y la mejor postura se
separa de la segunda por un margen configurable. En otro caso el resultado es
`ambiguous` o `unknown`.

La ausencia no es contradiccion:

```text
missing face       != face contradice la postura
partial leg        != persona invalida
low mask coverage  -> reduce peso de esa parte
two observed facts that disagree -> conflict explicito
```

## 4. Firmas que se evaluaran primero

- `acostado`: torso y piernas proximos a cama; cabeza y brazos mas alejados del
  torso; extension corporal horizontal.
- `sentado`: cabeza, hombros y brazos en cama; cadera hacia `bed/body`; piernas
  hacia `bed/feet`; torso relativamente compacto.
- `sentado-borde`: torso en cama; piernas parcialmente fuera o intersectando
  cama/piso; cabeza mas alejada del torso; cobertura de piernas parcial posible.
- `parado-aside`: torso y piernas en una banda de profundidad comun asociada al
  piso; piernas verticales; la cara es refuerzo y no requisito.
- `leaving-bed-aside-head`: torso y piernas aun en cama; cabeza mas alejada y
  torso inclinado hacia la salida.
- `foot-left-bed-2`: torso en cama, una extremidad inferior fuera y cobertura
  asimetrica; la cabeza es evidencia debil.
- `foots-left-bed-1`: torso inclinado, cabeza fuera y piernas con cobertura
  asimetrica o parcialmente fuera.

## 5. Hallazgos que cambian el diseno

1. Un keypoint puede ser un outlier: en `sentado-borde-1` una rodilla dio
   `1.452`, mientras el area de la pierna dio `1.127`. Se prioriza el area
   corporal con mascara sobre el punto aislado.
2. `in_envelope` contra la calibracion vacia no es un gate suficiente cuando el
   contenido de la escena induce deriva. Se conserva como evidencia auditable,
   pero la postura usara tambien referencias same-run, geometria e interseccion.
3. La cara puede faltar o estar fuera de las zonas. No se exige face para
   clasificar `parado-aside` si torso, piernas y geometria tienen consenso.
4. La segmentacion puede tener componentes o bbox mas amplios que el cuerpo. Se
   usa `mask_coverage` por parte y calidad; el area global no es una orden.
5. Una nueva captura no debe reescribir automaticamente las firmas. Primero se
   analiza, se audita el JSON y luego una persona decide si actualiza el TOML.

## 6. Limites

- Una muestra por postura no alcanza para fijar umbrales clinicos.
- No se afirma escala fisica sin referencia externa.
- No se clasifican multiples personas en el primer corte.
- No se integra con FSM ni `mana-control` en este sprint.
- La persistencia temporal queda como una etapa posterior al replay offline.
