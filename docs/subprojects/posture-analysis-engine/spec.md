# Especificacion: Engine de Analisis de Postura por Firmas

**Identificador:** POSTURE-ENGINE-001
**Version:** 0.2
**Estado:** contrato operativo offline por imagen

## 1. Alcance

El engine consume reportes de percepcion ya producidos y compara sus
observaciones con perfiles TOML de postura. No ejecuta modelos, no actualiza el
FSM y no transforma una profundidad monocular en distancia fisica.

La implementacion actual soporta una persona por imagen y las siete posturas
definidas en el README del subproyecto. El bin de posture-analysis consume
reportes ya generados; no recibe una imagen directamente.

## 2. Entradas

### 2.1 Perfil maestro

El TOML padre fija el contexto de la matriz:

```toml
schema_version = 1
engine = "posture-analysis"
model_key = "depth-l-640"
depth_semantics = "model-relative"
surface_calibration = "surface-calibration.toml"
posture_dir = "."
min_observed_components = 2
min_total_score = 0.55
semantic_min_total_score = 0.40
ambiguity_margin = 0.10
surface_spatial_padding_px = 48.0
surface_depth_padding_m = 0.15

[policy]
missing_is_conflict = false
allow_partial_parts = true
require_same_model_context = true
require_same_roi = true

[[postures]]
id = "acostado-1"
label = "acostado"
file = "acostado-1.toml"
weight = 1.0
```

El padre debe incluir una entrada `[[postures]]` por cada perfil activo. Los
paths de los perfiles se resuelven relativos al directorio del padre.

### 2.2 Perfil de postura

Cada postura es un TOML separado. Los valores de calibracion son centros y
tolerancias blandas, no gates exactos:

```toml
schema_version = 1
posture_id = "sentado-borde-1"
label = "sentado-borde"
training_sample = "sentado-borde-1"
base_posture = "sentado-aside"
plane = "aside-bed"

[policy]
min_observed_features = 3
min_observed_components = 2
allow_partial = true

[[features]]
id = "geometry.torso_tilt_deg"
source = "geometry"
field = "torso_tilt_deg"
center = 27.2
tolerance = 10.0
weight = 1.0
required = false

[[features]]
id = "body_part.torso.relative_to_torso"
source = "body_part"
part = "torso"
field = "relative_to_torso"
center = 0.0
tolerance = 0.15
weight = 1.0
required = true

[[features]]
id = "face.zone"
source = "face"
field = "zone"
allowed = ["bed/body", "floor/head"]
weight = 0.4
required = false
```

`base_posture` y `plane` separan la variante de referencia de la semantica
estable de una camara fija. Las bases iniciales son `acostado/in-bed`,
`sentado-in-bed/in-bed`, `sentado-aside/aside-bed` y
`standby-aside/aside-bed`. La clasificacion exacta usa `min_total_score`; el
consenso semantico puede usar `semantic_min_total_score` cuando una variante
concreta queda por debajo del umbral pero la postura base sigue siendo clara.

### 2.3 Par de reportes por imagen

El bin requiere dos JSON del mismo frame y contexto:

```text
radio.json: model_key, image, depth_roi, person_bbox y, cuando existen,
            keypoints, face, segment y profundidad puntual.
parts.json: model_key, image y actors[0].parts con geometria, calidad,
            cobertura de mascara y depth por parte.
```

`radio.json` debe contener `person_bbox` y `depth_roi` validos. El engine usa
el primer actor de `parts.json`; el primer corte no clasifica multiples actores.
Los reportes se deben producir con la misma imagen, sesion, modelo y ROI.

Las features numericas usan soporte triangular:

```text
support = clamp(1 - abs(observed - center) / tolerance, 0, 1)
```

Una feature categorica vale `1` si el valor esta en `allowed`. Cuando la fuente
es una zona calibrada y el valor pertenece a otra zona de la misma superficie,
aporta `0.5` como evidencia adyacente; no se crea un hueco artificial entre
`head`, `body` y `feet`. Una feature ausente no aporta ni penaliza.

## 3. Fuentes y estados

El adaptador normaliza los reportes actuales a estas fuentes:

| Fuente | Observaciones |
|---|---|
| `geometry` | bbox de persona, aspect, inclinacion, extension y angulos |
| `keypoint` | punto normalizado, confianza y depth por joint |
| `keypoint_group` | resumen de cabeza, hombros, cadera, rodillas y tobillos |
| `face` | bbox, confianza, depth, rango y zona |
| `segment` | bbox, area, componentes, area ratio y cobertura |
| `body_part` | geometria, quality, mask coverage y depth estadistico |
| `surface` | anclas espaciales, zona calibrada, padding y ajuste de profundidad |

Cada observacion tiene uno de estos estados:

```text
observed  dato valido y fresco
partial   dato valido pero incompleto o con cobertura limitada
missing   fuente o componente no disponible
invalid   dato presente pero no utilizable
stale     dato heredado de otro frame y fuera de TTL
conflict  dos fuentes observadas contradicen una relacion
```

`missing`, `invalid` y `stale` no se convierten silenciosamente en
`conflict`. La calidad de la fuente y la calidad del acuerdo son dimensiones
separadas.

## 4. Reglas de partes parciales

El engine debe tolerar las siguientes situaciones:

### Cabeza

- face + keypoints + mascara: soporte completo.
- sin face, con al menos dos joints de cabeza: usar region de pose y marcar
  `partial`.
- solo un joint: evidencia puntual de baja calidad.
- sin face y sin joints: `missing`, no falla el actor completo.

### Torso

- hombros y caderas: poligono completo.
- falta un lado: poligono parcial o bbox derivado, calidad reducida.
- no hay hombros ni caderas: torso `missing`.

### Brazos y piernas

- cada lado se evalua independientemente.
- un lado ausente no elimina el otro lado.
- codo o rodilla faltante reduce la geometria, pero no invalida el segmento si
  los extremos y la mascara dan soporte.

### Mascara

- la mascara se usa para recortar y medir cobertura.
- no se modifica la mascara cruda.
- una cobertura baja reduce el peso de la parte.
- una mascara ausente permite una geometria sin clip, pero el JSON debe
  explicitar `mask_missing`.

### Profundidad

- se acepta solo si hay pixeles validos suficientes.
- se conserva mediana, p10, p90, cobertura y relacion al torso.
- no se exige profundidad para cada parte si geometria 2D y otras partes son
  suficientes.

### Superficie espacial

La calibracion divide la escena en zonas `bed/head`, `bed/body`, `bed/feet`,
`floor/head`, `floor/body` y `floor/feet`. El engine proyecta las anclas de
cabeza, torso y caderas sobre esos poligonos y conserva:

- soporte de cama y de piso por ancla;
- indice blando de cuadrante `head=0`, `body=0.5`, `feet=1`;
- ajuste de profundidad a partir de `zone` y `delta_m` de radio;
- zona ganadora para auditoria.

Los limites se expanden con `surface_spatial_padding_px` (`48 px` en
`l-640`) para cubrir bordes y pequeñas diferencias entre bbox, keypoints y
mascara. `surface_depth_padding_m` (`0.15 m`) solo suaviza la confianza; no es
un gate absoluto porque la profundidad del cuerpo puede derivar respecto al
fondo calibrado.

## 5. Scoring y consenso

Cada perfil funciona como un evaluador independiente. El engine normaliza cada
feature, calcula su soporte y ordena todos los candidatos por score. No hay una
red neuronal ni un score de probabilidad.

### 5.1 Soporte de una feature

Para una feature numerica:

```text
support = clamp(1 - abs(observed - center) / tolerance, 0, 1)
```

Para una feature categorica:

```text
valor permitido                 -> support 1.0
otra zona de la misma superficie -> support 0.5
categoria no permitida           -> support 0.0
```

Una feature `missing` no aporta. Una feature `invalid` o `stale` no aporta. Una
feature `conflict` no aporta y marca el candidato como conflictivo.

### 5.2 Calidad, pesos y atencion

Cada feature tiene un peso `w`, una calidad `q` en `[0, 1]` y un peso de
atencion `a` determinado por su grupo:

```text
head    -> 1.50
torso   -> 1.35
legs    -> 1.00
surface -> 0.80
other   -> 0.90
```

El aporte de una feature observada es:

```text
numerator   += w * support * q * a
denominator += w * a
effective_weight = w * q * a
```

El score final es:

```text
score = sum(numerator) / sum(denominator)
```

El score por componente y los reportes de atencion se conservan por separado
para explicar el resultado. La calidad reduce el aporte de una observacion,
pero no convierte una observacion parcial en una ausencia silenciosa.

Los grupos de atencion actuales son `head`, `torso`, `legs`, `surface` y
`geometry`. `surface` se registra como evidencia del componente de depth en el
score por perfil, pero conserva su identidad espacial en el JSON.

### 5.3 Quorum y decision exacta

Para cada perfil se calculan:

- features observadas;
- componentes con al menos una feature observada;
- score total;
- `quorum`;
- features faltantes, parciales y conflictivas;
- razones por feature y por candidato.

El quorum de un perfil requiere simultaneamente:

1. `observed_features >= profile.policy.min_observed_features`;
2. `observed_components >= max(profile.policy.min_observed_components, master.min_observed_components)`;
3. ninguna feature requerida ausente, invalida, stale o conflictiva.

Despues de ordenar los candidatos, el margen exacto es:

```text
margin = best.score - second.score
```

Si no existe un segundo candidato, el margen es `best.score`. La decision
exacta se calcula en este orden:

1. contexto incompatible -> `incompatible`;
2. sin quorum, score menor que `min_total_score` o conflicto -> `unknown`;
3. margen menor que `ambiguity_margin` -> `ambiguous`;
4. en otro caso -> `classified`.

Solo `classified` publica `posture_id`, `label`, `base_posture` y `plane` en
la decision exacta. Los estados restantes conservan score, margen y evidencia
para auditoria.

### 5.4 Consenso semantico

El consenso agrupa los candidatos por `(base_posture, plane)`. Para cada grupo:

- conserva el mayor score de sus variantes;
- conserva quorum si alguna variante del grupo tiene quorum;
- conserva las variantes fuente;
- conserva conflictos de la variante que aporta el score mayor.

La decision semantica usa el mismo margen y las mismas reglas de compatibilidad
y conflicto, pero compara contra `semantic_min_total_score` y publica solo
`base_posture` y `plane`. Por eso una variante exacta puede quedar `unknown` o
`ambiguous` mientras el grupo semantico queda `classified`, si el grupo tiene
quorum y score semantico suficiente.

Si dos grupos semanticos quedan cerca, el resultado semantico es `ambiguous`;
no se fuerza una postura base.

## 6. Prioridad de senales

La matriz de perfiles fija que features participan y con que peso. La
implementacion agrega una atencion tecnica por grupo, pero no reemplaza los
pesos de los perfiles:

```text
head 1.50 > torso 1.35 > legs 1.00 > geometry 0.90 > surface 0.80
```

La geometria de torso/caderas y la superficie siguen siendo anclas de contexto:
la implementacion no usa una mediana absoluta de depth ni `in_envelope` como
gate universal. La ausencia de face no invalida un frame si keypoints, torso,
partes y depth alcanzan quorum. Las features marcadas `required` fijan la
evidencia troncal; las restantes refinan el ranking.

El consenso agrupa variantes por postura base y plano. No es persistencia
temporal ni una segunda red de clasificacion.

## 7. Salida JSON

Cada imagen produce un documento autocontenido:

```json
{
  "schema_version": 1,
  "engine": "posture-analysis",
  "image": "sentado-borde-1.jpeg",
  "model_key": "depth-l-640",
  "depth_semantics": "model-relative",
  "calibration": {
    "session": "...",
    "roi": [452, 140, 1300, 1029],
    "surface_spatial_padding_px": 48.0,
    "surface_depth_padding_m": 0.15,
    "compatible": true
  },
  "decision": {
    "status": "classified",
    "posture_id": "sentado-borde-1",
    "label": "sentado-borde",
    "base_posture": "sentado-aside",
    "plane": "aside-bed",
    "score": 0.78,
    "margin": 0.19,
    "observed_components": 4
  },
  "semantic_decision": {
    "status": "classified",
    "base_posture": "sentado-aside",
    "plane": "aside-bed",
    "score": 0.78,
    "margin": 0.21
  },
  "candidates": [
    {
      "posture_id": "sentado-borde-1",
      "label": "sentado-borde",
      "score": 0.78,
      "components": {
        "geometry": {"status": "observed", "score": 0.84},
        "body_parts": {"status": "partial", "score": 0.79},
        "depth": {"status": "observed", "score": 0.72},
        "face": {"status": "observed", "score": 0.66}
      },
      "reasons": ["torso_on_bed", "legs_cross_surface_boundary"]
    }
  ],
  "observations": {},
  "missing": [],
  "conflicts": []
}
```

Los campos de ejemplo no autorizan esos valores; solo fijan la forma del
contrato. El JSON debe contener suficiente detalle para explicar por que una
fuente no participo.

## 8. Compatibilidad de contexto

La implementacion actual marca `incompatible` cuando difieren:

- `model_key` de `radio.json` respecto al master;
- `model_key` de `parts.json` respecto al master;
- `depth_roi` del radio respecto a `SurfaceCalibration.roi`;
- ROI de depth de una parte respecto a la ROI del radio, cuando el campo existe.

El master tambien valida que la calibracion de superficies use el mismo
`model_key`. El fingerprint, dimensiones de frame y nombre de sesion se
conservan en los artefactos, pero no son gates implementados aun en el
adaptador v0.2. Cambiar modelo, ROI o camara requiere de todos modos una nueva
matriz de firmas y una nueva calibracion.

## 9. No objetivos del primer corte

- No ejecutar inferencia.
- No aprender o modificar perfiles automaticamente.
- No usar una red neuronal nueva para postura.
- No publicar postura en el FSM.
- No clasificar multiples personas.
- No suavizar entre frames hasta que exista un replay temporal separado.
- No convertir `depth_m` en distancia fisica sin ground truth.

Los campos de politica `missing_is_conflict`, `allow_partial_parts` y
`allow_partial` forman parte del schema y se validan/cargan, pero el
comportamiento v0.2 se determina por estados de observacion, quorum, calidad y
features `required`. No deben interpretarse como switches clinicos hasta que
exista una implementacion especifica y sus pruebas.

## 10. Criterios de aceptacion

- Las siete muestras producen un JSON explicable y determinista.
- Un reporte sin face sigue pudiendo producir un candidato si las partes
  restantes tienen quorum.
- Un solo keypoint contradictorio no vence a un area corporal completa.
- Una mascara parcial reduce score y aparece en las razones.
- Dos candidatos cercanos producen `ambiguous`, no una eleccion arbitraria.
- Evidencia insuficiente produce `unknown`.
- Contexto de modelo/ROI incompatible se reporta de forma explicita.
- Repetir el mismo reporte produce el mismo JSON semantico.
- El engine no modifica los reportes de inferencia ni la sesion maestra.

## 11. Operacion reproducible

El procedimiento normativo para una imagen esta en [manual.md](manual.md).
Cada prueba persistente debe guardar sus artefactos en:

```text
runs/<run-id>/<frame-id>/
```

Como minimo se conservan `radio.json`, `parts.json` y `posture.json`. Los
previews y el resumen reducido son recomendados para revision humana. No se
usan `/tmp` ni archivos de salida compartidos entre pruebas.

El flujo es:

1. ejecutar `deep-calib-radio` sobre el JPEG;
2. ejecutar `deep-calib-parts` sobre el mismo JPEG y sesion;
3. ejecutar `posture-analysis` con ambos JSON y el master correspondiente;
4. revisar `.decision`, `.semantic_decision`, `.candidates`, `missing` y
   `conflicts`;
5. validar JSON y guardar el resultado bajo `runs/`.

El comando `posture-analysis` sin `--radio` y `--parts` solo valida y reporta
la matriz de perfiles; no produce una postura.
