# Manual operativo: Engine de Analisis de Postura

**Version:** 0.1
**Estado:** operativo offline por imagen
**Modelo de referencia:** `depth-l-640`

## 1. Proposito

Este manual explica como tomar una imagen, generar los reportes de percepcion y
obtener dos resultados:

- decision exacta: la variante de perfil que mejor coincide;
- decision semantica: la postura base y el plano de la escena.

El engine es offline. El bin `posture-analysis` no recibe un JPEG ni ejecuta
modelos; recibe los dos JSON producidos por `deep-calib-radio` y
`deep-calib-parts`.

```text
JPEG
  |
  +--> deep-calib-radio --> radio.json
  |
  +--> deep-calib-parts --> parts.json
                              |
radio.json + parts.json + master.toml
                              v
                       posture-analysis
                              |
                         posture.json
```

La salida es diagnostica. No modifica el FSM, `mana-control`, los perfiles ni
la calibracion.

## 2. Contexto obligatorio

Para la matriz actual se debe usar el mismo contexto en los tres pasos:

```text
modelo:       depth-l-640
master:       config/posture-analysis/l-640/master.toml
calibracion:  config/workshop/deep-calib-depth-l-640.toml
ROI:          [452, 140, 1300, 1029]
```

No mezclar:

- `depth-l-640` con reportes `depth-l-320` o `depth-standard`;
- reportes de otra ROI;
- reportes de otro frame;
- JSON de una corrida anterior sin comprobar el contexto.

El frame debe contener una sola persona visible en la escena calibrada. Si no
hay deteccion de persona, `radio.json` no tendra `person_bbox` y el analisis no
puede construir una decision.

## 3. Directorio de cada prueba

Cada prueba se guarda bajo `runs/`. No usar `/tmp` para resultados del engine.
No sobrescribir una prueba anterior.

```text
runs/
  run_003/
    frame_0001/
      input.path.txt
      input.sha256
      radio.json
      radio.png
      parts.json
      parts.png
      posture.json
      decision-summary.json
```

`run_003` identifica una sesion de trabajo. `frame_0001` identifica la imagen
analizada dentro de esa sesion. Se pueden usar nombres descriptivos si el
frame no pertenece a una secuencia.

Los JPEG originales no se renombran ni se modifican. `input.path.txt` registra
su ubicacion y `input.sha256` permite comprobar que no cambio.

## 4. Analizar un frame

Ejecutar desde la raiz del repositorio:

```bash
RUN="runs/run_003/frame_0001"
IMAGE="samples/frame_0001.jpeg"
MASTER="config/posture-analysis/l-640/master.toml"
SESSION="config/workshop/deep-calib-depth-l-640.toml"

mkdir -p "$RUN"
printf '%s\n' "$IMAGE" > "$RUN/input.path.txt"
sha256sum "$IMAGE" > "$RUN/input.sha256"
```

### 4.1 Reporte de radio

Este paso ejecuta depth de escena, deteccion, pose, face y, con `--seg`,
segmentacion. Produce bbox de persona, keypoints, face, zonas y profundidad
puntual.

```bash
cargo run --release --bin deep-calib-radio -- \
  --config config/mana.toml \
  --session "$SESSION" \
  --image "$IMAGE" \
  --json "$RUN/radio.json" \
  --output "$RUN/radio.png" \
  --seg
```

### 4.2 Reporte de partes corporales

Este paso ejecuta el `BodyPartsEstimator` sobre el mismo frame y contexto.
Produce geometria, calidad, cobertura de mascara y depth por parte.

```bash
cargo run --release --bin deep-calib-parts -- \
  --config config/mana.toml \
  --session "$SESSION" \
  --image "$IMAGE" \
  --json "$RUN/parts.json" \
  --output "$RUN/parts.png"
```

### 4.3 Decision de postura

El engine carga el master y todos sus perfiles, normaliza los dos reportes,
evalua cada perfil y calcula las dos decisiones.

```bash
cargo run --release --bin posture-analysis -- \
  --master "$MASTER" \
  --radio "$RUN/radio.json" \
  --parts "$RUN/parts.json" \
  --json "$RUN/posture.json"
```

El bin imprime el JSON y tambien lo escribe en `posture.json`.

## 5. Leer la respuesta

Para ver solo la respuesta relevante:

```bash
jq '{
  imagen: .image,
  exacta: .decision,
  semantica: .semantic_decision
}' "$RUN/posture.json" | tee "$RUN/decision-summary.json"
```

### 5.1 Decision exacta

La decision exacta vive en `.decision`.

```json
{
  "status": "classified",
  "posture_id": "acostado-1",
  "label": "acostado",
  "base_posture": "acostado",
  "plane": "in-bed",
  "score": 0.568,
  "margin": 0.174,
  "observed_components": 3
}
```

`posture_id` identifica la variante de perfil. Por ejemplo, dos perfiles
pueden compartir la base `acostado` pero representar posiciones distintas de
piernas o cabeza.

### 5.2 Decision semantica

La decision semantica vive en `.semantic_decision`.

```json
{
  "status": "classified",
  "base_posture": "acostado",
  "plane": "in-bed",
  "score": 0.568,
  "margin": 0.357
}
```

La semantica no elige un `posture_id`. Agrupa variantes por
`base_posture + plane` y conserva la mejor evidencia de ese grupo.

### 5.3 Estados

| Estado | Significado |
|---|---|
| `classified` | Hay quorum, score suficiente y separacion del segundo candidato. |
| `ambiguous` | Hay quorum y evidencia, pero los dos mejores candidatos estan demasiado cerca. |
| `unknown` | Falta quorum, el score es insuficiente o existe un conflicto observado. |
| `incompatible` | El modelo o la ROI de los reportes no coincide con la matriz. |

Cuando el estado no es `classified`, los campos de identidad como `label`,
`posture_id` o `base_posture` quedan en `null`. El score, margen, faltantes y
conflictos siguen siendo utiles para auditar el motivo.

## 6. Auditar por que decidio

Ver los candidatos exactos ordenados:

```bash
jq '[.candidates[] | {
  posture_id,
  label,
  base_posture,
  plane,
  score,
  quorum,
  observed_components,
  missing,
  conflicts,
  reasons
}]' "$RUN/posture.json"
```

El margen se publica en `.decision.margin`; los candidatos individuales no
guardan el margen calculado contra el siguiente.

Ver las observaciones de superficie, que son especialmente importantes para
separar `in-bed` de `aside-bed`:

```bash
jq '.observations | with_entries(select(.key | startswith("surface."))) \
  | to_entries' "$RUN/posture.json"
```

Ver faltantes y conflictos globales:

```bash
jq '{missing, conflicts, calibration}' "$RUN/posture.json"
```

Las razones por feature estan dentro de:

```text
.candidates[].features[]
```

Cada feature conserva `observed`, `support`, `quality`, `weight`,
`effective_weight` y `reason`.

## 7. Validar la matriz sin analizar una imagen

Para validar que el master, la calibracion y todos los perfiles cargan:

```bash
mkdir -p runs/run_003
cargo run --release --bin posture-analysis -- \
  --master config/posture-analysis/l-640/master.toml \
  --json runs/run_003/profile-matrix.json
```

Este comando no genera una postura. Solo informa el contexto y la matriz de
perfiles.

## 8. Validacion de una prueba

```bash
jq empty "$RUN/radio.json" "$RUN/parts.json" "$RUN/posture.json"
cargo test -p mana-lite --lib posture_analysis
```

Antes de comparar dos resultados comprobar:

```bash
jq '{image, model_key, depth_roi}' "$RUN/radio.json"
jq '{image, model_key, zones: [.zones[] | {zone, calibrated_m}]}' "$RUN/parts.json"
jq '.calibration' "$RUN/posture.json"
```

La comparacion solo es valida si los modelos, ROI y sesion son compatibles.

## 9. Que no hace este flujo

- No publica la postura en el FSM.
- No controla `mana-control`.
- No mantiene identidad ni persistencia entre frames.
- No clasifica multiples personas.
- No aprende perfiles a partir de una captura.
- No convierte `depth_m` en distancia fisica garantizada.
- No reemplaza una validacion clinica.

## 10. Problemas frecuentes

### Falta `person_bbox`

El reporte de radio no encontro una persona util. Revisar la imagen, el modelo
y el recorte antes de ejecutar `posture-analysis`.

### Estado `incompatible`

Revisar `model_key` y `depth_roi` de `radio.json` y `parts.json`. Para esta
matriz ambos deben corresponder a `depth-l-640` y ROI `[452, 140, 1300, 1029]`.

### Estado `unknown`

No significa un error de ejecucion. Significa que el engine no encontro
evidencia suficiente para una decision segura. Revisar `missing`, `conflicts`,
`quorum` y las features requeridas.

### Estado `ambiguous`

Hay evidencia suficiente, pero la diferencia entre los dos primeros perfiles es
menor que `ambiguity_margin`. No reemplazarlo automaticamente por el primer
perfil.

### Resultado exacto `unknown` y semantico `classified`

Es un resultado esperado cuando una variante concreta no supera
`min_total_score`, pero otra variante de la misma postura base si supera
`semantic_min_total_score` y conserva quorum.
