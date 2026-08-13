# Memoria spec — comportamiento del modelo de profundidad (yolo26 depth)

Estado: borrador vivo · 2026-08-13 · herramientas: `deep-calib`, `deep-calib-preview`, `deep-calib-pose`, `deep-calib-matrix`

## Contexto

- Cámara cenital/picada sobre una cama; frame 1920x1080; ROI de escena `[560, 140, 1240, 820]` (catálogo `depth-standard` = yolo26l-depth-fp16-320).
- Sesión `config/deep-calib.toml`: 6 zonas poligonales, 1 muestra por zona (`frame_samples = 1`, `mad_depth = 0.0` — todavía no es estadística útil).
- Medianas calibradas (m): bed/head **3.356**, bed/body **2.603**, bed/feet **2.296** · floor/head **4.724**, floor/body **3.683**, floor/feet **3.232**.
- Imágenes de prueba: `frame.jpeg` (cama vacía) y `acostado-1.jpeg` (persona acostada).

## Hechos observados

1. **Dentro de una corrida, la mediana por zona es estable**: mismo frame.jpeg → delta 0.000 en las 6 zonas, valid_ratio 1.000. El rango min/max NO es estable (frame.jpeg dio `2.02–3.78` y `2.19–8.70` en corridas distintas). La mediana es la señal; el rango, ruido.

2. **El modelo es determinista**: mismo frame + mismo modelo ⇒ mismas medianas (verificado 2026-08-13 con los 8 modelos sobre frame.jpeg, `deep-calib-matrix` ×2 con diff vacío). Las variaciones de rango min/max vienen de la escena, no del runtime.
3. **Deriva de escala por contenido de escena**: con acostado-1 (persona en cama), las 3 zonas de piso se corrieron +0.84 / +1.00 / +1.26 m contra la calibración, y la cama +0.11..0.27. El piso no cambió físicamente ni está ocluido → la presencia de la persona cambia la lectura del modelo (deriva condicionada al contenido), no es ruido entre corridas. Comparar mediciones de escenas distintas mezcla esta deriva.
4. **Sesgo de cabeza (candidato a limitación del modelo)**: con la persona acostada, la cabeza lee ~4.0 m — nose 4.068, bbox de cara mediana 4.052 (p10 3.907, p90 5.611, valid 1.000) — mientras la cama bajo la cabeza (bed/head observada) lee 3.467. En cámara cenital, una cabeza sobre la cama **no puede leer más lejos** que la superficie que la sostiene (elevación ⇒ menor profundidad). Lectura 0.6 m más lejana = patrón clásico de fusión pelo oscuro / pared de fondo en depth aprendido.

5. **Clasificación contra referencia equivocada sesga**: asignar keypoints por mediana *calibrada* (otra escena) arrastra la deriva de escala. Con referencia *same-run* (medianas observadas del mismo run), hombros/codos/caderas/rodillas clasifican a bed correctamente. La cabeza persiste en floor/feet (4.052 vs 4.070 observada — coincidencia casi exacta): la deriva de escala no explica sola el sesgo de cabeza.

## Invariantes físicas (expectativas de la escena)

- `bed < floor` siempre: la cama elevada está más cerca de la cámara.
- Las zonas de piso **no cambian** por la presencia de la persona (no ocluidas) → shift > ~0.1 m en piso = deriva del modelo.
- El cuerpo acostado lee entre bed/feet y bed/head según posición; rodillas/feet al pie de cama, cabeza a la cabecera.
- La cabeza apoyada o levantada lee `≤` superficie de cama (nunca más lejos).

## Contratos adoptados

- **Clasificación**: contra medianas observadas del mismo run (referencia autoconsistente). Las calibradas solo auditan deriva (columna `delta_m` de las tablas).
- **Determinismo**: verificado — mismo input + mismo modelo ⇒ mismas medianas de zona (8 modelos × frame.jpeg, 2 corridas). Los cambios de lectura entre escenas son del modelo frente al contenido, no del runtime.

## Preguntas abiertas

1. ¿`depth-person` (crop de persona) lee la cabeza en 3.4–3.6 m? → parcialmente: en el crop la cabeza cae en el extremo cercano del sobre persona+cama (≈3.77 m en escala l-320, Δ+0.30 vs +0.59 de escena); sigue con sesgo, pero ya no vuela al piso.
2. ¿imgsz 640 o modelo l/x reducen el sesgo de cabeza y la deriva de piso? → l-640: la deriva de piso sí (mejor de la matriz); el sesgo de cabeza relativo baja a ~14% (mínimo).
3. ¿El no-determinismo entre corridas desaparece con half off / imgsz fijo? → no hay no-determinismo: el modelo es determinista; la deriva observada es por contenido de escena.

## Workshop (deep-calib-matrix)

- Matriz: modelos depth **s / m / l / x** × **320 / 640** × {cama vacía, persona}.
- Métricas por modelo:
  - (a) orden y separación de zonas (bed < floor, gradiente head→feet coherente);
  - (b) deriva de piso entre imágenes (mínima = más estable);
  - (c) head probe: mediana del bbox de cara vs bed/head observada (cerca = lee bien la cabeza).
- Criterio de "más preciso" para runtime: menor deriva de piso + cabeza cerca de bed/head + determinismo. El ganador no necesariamente corre siempre — es una decisión de deploy informada por esta evidencia.

## Siguientes pasos

- Correr `deep-calib-matrix` sobre frame.jpeg + acostado-1.jpeg y comparar la matriz.
- Recalibrar la sesión con el modelo elegido y N frames (estadística real: mad, p10/p90).
- Revisar determinismo (half on/off) y, si el sesgo de cabeza persiste, evaluar `depth-person` para la cabeza y mantener depth-standard como contexto de escena.

## Resultados del workshop (2026-08-13)

Matriz 8 modelos × 2 imágenes (`deep-calib-matrix`). Zonas en metros; `face_m` = mediana del bbox de cara sobre acostado-1; `Δface` = face_m − bed/head observada.

| modelo | image | bed/head | bed/feet | floor/head | floor/feet | Δfloor* | face_m | Δface |
|---|---|---|---|---|---|---|---|---|
| s-320 | frame / acostado | 1.577 / 1.488 | 1.134 / 1.115 | 2.547 / 2.783 | 1.511 / 1.605 | +0.09..+0.24 | 1.859 | +0.37 |
| m-320 | | 2.110 / 2.828 | 1.567 / 1.954 | 3.934 / 4.672 | 2.535 / 2.989 | +0.45..+0.74 | 3.564 | +0.74 |
| l-320 (calibrado) | | 3.356 / 3.467 | 2.296 / 2.567 | 4.724 / 5.982 | 3.232 / 4.070 | **+0.84..+1.26** | 4.052 | +0.59 |
| x-320 | | 2.412 / 2.848 | 1.938 / 2.096 | 4.682 / 4.928 | 3.164 / 3.290 | +0.13..+0.25 | 3.391 | +0.54 |
| s-640 | | 1.056 / 1.083 | 0.737 / 0.773 | 1.369 / 1.514 | 0.960 / 1.014 | +0.05..+0.15 | 1.491 | +0.41 |
| m-640 | | 1.058 / 1.361 | 0.727 / 0.825 | 1.506 / 2.046 | 0.917 / 1.233 | +0.32..+0.54 | 1.706 | +0.35 |
| l-640 | | 1.285 / 1.255 | 0.867 / 0.850 | 2.043 / 2.125 | 1.322 / 1.378 | **+0.06..+0.08** | 1.427 | **+0.17** |
| x-640 | | 1.519 / 1.256 | 0.990 / 0.808 | 2.376 / 2.257 | 1.431 / 1.315 | −0.12..−0.13 | 1.465 | +0.21 |

*Δfloor = shift de la zona de piso con más deriva entre ambas imágenes.

### Lectura

1. **La escala absoluta es específica de cada modelo**: la misma cama lee 2.3–3.4 m (l-320), 1.1–2.5 (s-320), 0.7–1.5 (s-640). La calibración solo es válida para el modelo exacto con el que se hizo; cambiar de modelo exige recalibrar. El orden relativo (bed < floor, gradiente head→feet) se preserva en todos.
2. **Estabilidad de piso (deriva entre imágenes)**: l-640 es la más estable (+0.06..0.08), s-640 y x-320 muy bien; l-320 (el calibrado) es la PEOR (+0.84..1.26).
3. **Sesgo de cabeza: sistemático en todos los modelos** (+14–38% relativo a bed/head): no es culpa de un modelo puntual, es de la arquitectura/tarea. Los relativamente mejores: l-640 (+0.17) y l-320 (+0.59 → 17% relativo).
4. **Las zonas se solapan en profundidad** (bed/head 3.356 vs floor/feet 3.232 en l-320): la clasificación por mediana absoluta es frágil en esa franja en cualquier modelo. Para runtime, la capa `bed` debe decidir por contexto geométrico + pose, no solo por mediana.

### Conclusión del workshop

- Para runtime conviene **l-640** (estabilidad + menor sesgo relativo de cabeza), con recalibración propia y validación del factor de escala (0.7–2 m parece cercano para cámara cenital — falta ground truth).
- El sesgo de cabeza no se resuelve eligiendo modelo; sigue pendiente `depth-person` sobre el crop para medir la cabeza del paciente.
- `l-320` (el actual) es el peor en estabilidad: recalibrar o migrar antes de confiar en deltas de deriva.

### Head probe con crop de persona (2026-08-13, mismo run)

Pipeline runtime (`detect-fast` → crop persona margen 0.20 → `depth-person-s-320`) sobre acostado-1. `deep-calib-person`:

- Persona detectada: bbox [752, 352, 1247, 964] conf **0.438** → el rule del runtime exige 0.50: en producción **este frame no correría depth-person** (a ajustar o aceptar).
- Crop persona (653,229)-(1346,1080); cara en crop (312,120)-(376,186).

| modelo | roi_min | roi_med | roi_max | face_m | face_p10 | face_p90 |
|---|---|---|---|---|---|---|
| depth-person-s-320 | 0.927 | 1.554 | 3.667 | **1.767** | 1.714 | 3.269 |
| depth-standard (escena) | — | — | — | 4.062 | 3.914 | 5.922 |

Lectura:

1. En el depth de crop, la cabeza cae en el **extremo cercano** del sobre persona+cama (rango 0.93–3.67): el modelo de crop resuelve la cabeza relativa al cuerpo/cama, no la manda al piso.
2. Escala a l-320 (factor s≈0.47×l por la cama: 1.577/3.356): cabeza ≈ **3.77 m** vs bed/head observada 3.467 → Δ+0.30, la mitad del sesgo del modelo de escena (Δ+0.59). La cabeza queda dentro del sobre de la cama, no en el piso.
3. `face_p90` 3.269: el bbox de cara barre mucho rango en crop depth (mezcla pelo/almohada/colchón) — el probe por bbox de cara es ruidoso en el crop; convendría un probe más chico (nariz de pose).
4. **Escalas incompatibles entre modelos** (factor 0.47): no mezclar valores absolutos de scene y person-crop. El diseño del blueprint (depth-standard = contexto, depth-person = evidencia relativa de partes) es el correcto: usar el crop solo en términos relativos.

### Geometría de cámara (clave para leer los datos)

- La cámara está montada en la **pared de los PIES de la cama**, picada hacia la cabecera: los pies de la cama son lo más CERCA de la cámara y la cabecera lo más LEJOS.
- Gradiente calibrado que lo confirma en todos los modelos: `bed/feet < bed/body < bed/head` (l-320: 2.30 / 2.60 / 3.36).
- Consecuencia: la cabeza ACOSTADA queda en el extremo lejano del ángulo, contra la pared de la cabecera → leer lejos (amarillo) es lo esperado físicamente (ángulo + foco + profundidad), no un error del modelo.
- Al INCORPORARSE, la persona se separa de la pared y el modelo bueno la lee sobre la cama (azul).

### Test de aceptación (criterio del taller, 2026-08-13)

El mismo cuerpo, dos poses: acostado lee lejos (amarillo), sentado lee sobre la cama (azul).

- **Test A — sentado**: cara + ambos hombros deben leer zona `bed` (el cuerpo fluye bed/head → bed/body → bed/feet).
- **Test B — acostado**: la cara debe leer MÁS LEJOS que la mediana observada de bed/head (zona floor).

Resultado por modelo (matriz completa y reproducible en `scripts/score-models.py`):

| modelo | A sentado | B acostado | veredicto |
|---|---|---|---|
| l-320 | si | si | ok |
| l-640 | si | si | ok |
| m-320 | **NO** | si | DESCARTAR |
| m-640 | **NO** | si | DESCARTAR |
| s-320 | si | si | ok |
| s-640 | si | si | ok |
| x-320 | si | si | ok |
| x-640 | si | si | ok |

m-320/m-640 fallan porque al sentarse cara y hombros siguen leyendo floor (piso) con caderas y rodillas en bed — incoherencia interna: un cuerpo sentado no puede tener hombros en el piso. El modelo `m` queda descartado como escena.

### Recomendación (2026-08-13, tras test de aceptación)

Métrica y costo (CPU, FP16, este equipo, frame.jpeg). Descartados m-320/m-640 por el test A.

| modelo | deriva piso | sesgo cabeza (rel.) | sentado Δface | latencia |
|---|---|---|---|---|
| s-320 | +0.09..0.24 | 25% | +0.10 | 64 ms |
| l-320 (actual) | +0.84..1.26 | 17% | +0.09 | 144 ms |
| **x-320 (recomendado)** | **+0.13..0.25** | **19%** | **+0.02** | 253 ms |
| s-640 | +0.05..0.15 | 38% | +0.03 | 223 ms |
| l-640 | +0.06..0.08 | 14% | +0.03 | 479 ms |
| x-640 | −0.12..−0.13 | 17% | +0.01 | 892 ms |
| ~~m-320~~ / ~~m-640~~ | — | — | falla test A | — |

- **Escena (recomendado): `depth-x-320`** (x, imgsz 320). Pasa ambos tests (sentado Δface +0.02 — impecable), mejor estabilidad de la familia 320 (piso casi tan estable como l-640), escala absoluta plausible (2.3–4.9 m para cámara cenital) y 2× más barato que l-640. Requiere **recalibrar la sesión** con este modelo y N frames.
- Alternativa de máxima precisión: `depth-l-640` (deriva mínima, mejor cabeza relativa) si la latencia de ~0.5 s se acepta y se valida su escala (0.7–2.0 m — sospechosamente comprimida).
- **Cabeza/paciente (evidencia relativa): `depth-person-s-320`** (crop de persona, s, 320) — ya es la del blueprint; bajo su umbral de confianza si el detect baja de 0.50.
- La escala absoluta no es métrica física confiable en NINGÚN modelo (factores 2–3× entre variants); usar siempre estructura relativa: mediana por zona same-run, ordering bed < floor, y evidencia relativa en el crop de persona.
- **El flip acostado→sentado es el discriminador de calidad**: cualquier modelo futuro debe pasar el test A/B (reproducible con `scripts/score-models.py` sobre `demo-deep-calib/results/*.json`).
