# Especificacion: Engine de Analisis de Postura por Firmas

**Identificador:** POSTURE-ENGINE-001
**Version:** 0.1
**Estado:** contrato de construccion offline

## 1. Alcance

El engine consume reportes de percepcion ya producidos y compara sus
observaciones con perfiles TOML de postura. No ejecuta modelos, no actualiza el
FSM y no transforma una profundidad monocular en distancia fisica.

La primera implementacion soporta una persona por imagen y las siete posturas
definidas en el README del subproyecto.

## 2. Entradas

### 2.1 Perfil maestro

El TOML padre fija el contexto de la matriz:

```toml
schema_version = 1
engine = "posture-analysis"
model_key = "depth-l-640"
depth_semantics = "model-relative"
surface_calibration = "config/workshop/deep-calib-depth-l-640.toml"
posture_dir = "config/posture-analysis/l-640"
min_observed_components = 2
min_total_score = 0.55
ambiguity_margin = 0.10

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

Las features numericas usan soporte triangular:

```text
support = clamp(1 - abs(observed - center) / tolerance, 0, 1)
```

Una feature categorica vale `1` si el valor esta en `allowed`, `0` si existe y
no coincide. Una feature ausente no aporta ni penaliza.

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
| `surface` | interseccion con bed/floor, residuo y referencia |

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

## 5. Scoring y consenso

El engine calcula scores por componente, no un unico score opaco:

```text
geometry_score
parts_score
depth_score
face_score
source_quality
```

Para cada componente disponible:

```text
component_score = sum(weight * support) / sum(weight disponible)
```

El score final renormaliza solo los componentes observados. Ademas conserva:

- `observed_components`.
- `missing_components`.
- `conflicts`.
- `effective_weight`.
- `coverage_quality`.

Una postura solo puede quedar `classified` cuando:

1. cumple `min_observed_components` del perfil y del padre;
2. su score supera `min_total_score`;
3. supera al segundo candidato por `ambiguity_margin`;
4. no tiene un conflicto critico marcado como requerido por el perfil.

Si hay evidencia suficiente pero dos candidatos estan cerca, el estado es
`ambiguous`. Si no hay evidencia suficiente, es `unknown`.

## 6. Prioridad de senales

La configuracion puede cambiar pesos, pero el diseno inicial recomienda:

```text
geometry/body_parts  >  depth relative  >  face alone
```

La cara es importante para `head`, no para decidir por si sola entre sentado,
acostado y parado. El consenso debe poder clasificar `parado-aside` aunque face
sea parcial o este fuera de las zonas.

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
    "compatible": true
  },
  "decision": {
    "status": "classified",
    "label": "sentado-borde",
    "score": 0.78,
    "margin": 0.19,
    "observed_components": 4
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

El engine rechaza el analisis o lo marca `incompatible` cuando difieren:

- `model_key` o fingerprint.
- ROI de depth.
- ancho o alto del frame.
- sesion de superficies requerida por el perfil.

Cambiar modelo, ROI o camara requiere una nueva matriz de firmas.

## 9. No objetivos del primer corte

- No ejecutar inferencia.
- No aprender o modificar perfiles automaticamente.
- No usar una red neuronal nueva para postura.
- No publicar postura en el FSM.
- No clasificar multiples personas.
- No suavizar entre frames hasta que exista un replay temporal separado.
- No convertir `depth_m` en distancia fisica sin ground truth.

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
