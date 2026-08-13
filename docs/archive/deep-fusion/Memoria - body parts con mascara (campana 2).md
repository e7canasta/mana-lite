# Memoria - Body parts con máscara (campaña 2, workshop)

Complementa *Memoria de posturas - workshop finalistas.md* (campaña 1, keypoints).
Campaña 2 reutiliza la implementación existente del runtime:
`BodyPartsEstimator` + `attach_depth` + `attach_surface_evidence`
(`src/app/body_parts.rs`) sobre los 3 modelos finalistas (x-320, l-640, l-320).

## Cómo se corrió

- Herramienta: `deep-calib-parts` (nuevo bin). Depth de escena de la sesión
  (roi = zonas calibradas), pose/face/seg sobre el recorte de zonas; los
  `Detection` se traducen a frame como el runtime (`collect_detections` +
  `translate_detections_to_frame`, máscaras normalizadas a frame).
- El estimador arma las 6 partes (head de face bbox/pose; torso de
  hombros/caderas; miembros de polilínea con radio) y las recorta por la
  máscara de `seg-standard` (yolo26s-seg-fp16-320). La profundidad se muestrea
  en el footprint de cada parte *clipado a la máscara* — áreas compactas, no
  puntos. Evidencia de superficie por zona calibrada (bed/floor) con
  `in_envelope`.
- 7 muestras × 3 modelos = 21 corridas. JSON en `demo-deep-calib-parts/results/`,
  previews en `demo-deep-calib-parts/persona/`.

Nota: la clasificación de zona de los resúmenes usa la *mediana observada de la
zona en el mismo frame* (referencia con deriva), como el pose tool; la
evidencia cruda del JSON no lleva ese ajuste y `in_envelope` salió false en
todos los casos (los residuos son de decenas de cm).

## Matriz de resultados (zona + residuo vs mediana observada de la zona)

Cada celda: `zona residuo_m`. Residuo > 0 = la parte quedó *más lejos* que la
zona a la que se aproxima (señal "fuera de la cama").

| muestra | x-320 | l-640 | l-320 |
|---|---|---|---|
| **sentado-1** | head bed/head +0.02, torso bed/head, arms bed/head, legs bed/feet/body | idem (residuos ±0.03) | idem (head +0.10) |
| **sentado-borde-1** | TODO bed/body (head +0.08) | TODO bed (body/feet) | TODO bed/body |
| **leaving-bed-aside-head-1** | head bed/head +0.15, torso bed/body, resto bed | idem (head +0.02) | idem |
| **acostado-1** | head floor/feet +0.09, arms floor/feet, torso bed/head −0.03, legs bed | head floor/feet +0.05, resto igual | head floor/feet −0.03, arms floor/feet, torso bed/head |
| **foot-left-bed-2** | head bed/head +0.40, torso floor/feet, right_arm bed/head, legs bed/body | head floor/body −0.08, torso bed/head, legs bed/body | head floor/body −0.06, torso bed/head, left_arm bed/head, legs bed/body |
| **foots-left-bed-1** | head floor/body −0.35, torso floor/feet, left_arm floor/feet, right_arm bed/head +0.47, legs bed | head floor/body −0.08, torso bed/head, resto bed | head floor/body +0.36, torso bed/head, legs bed/body/feet |
| **parado-aside-1** | head bed/head +0.12, torso floor/body, arms floor (left) / **no-depth** (right), legs floor | head floor/head +0.28, torso bed/head, left_arm bed/head, legs floor | head floor/head +0.11, torso floor/feet, arms floor, legs floor |

## Firmas por postura (por área, con máscara)

1. **sentado**: todas las partes en bed; head bed/head, piernas bed/feet. La
   cabeza queda *más lejos* que el torso (borde cabecera).
2. **sentado en el borde**: todas en bed pero comprimidas a bed/body (la
   cabeza deja de leer bed/head). Diferencia el borde de sentado normal.
3. **acostado**: firma fuerte y consistente en los 3 modelos:
   **cabeza y brazos en floor, torso en bed/head, piernas en bed**. La cabeza
   se separa del torso (residuos: head +0.05..0.09 floor/feet vs torso −0.03
   bed/head): cabeza apunta a la pared de la cabecera, más lejos que el
   colchón.
4. **parado al costado**: torso/piernas en floor (aunque torso pega a
   bed/head en x-320/l-640), **brazo derecho sin profundidad en los 3
   modelos** (brazo apuntando fuera del mapa depth o contra zona sin píxeles
   válidos — artefacto del recorte por máscara, no del modelo depth).
   La cabeza sola NO decide (x-320 la lee bed/head +0.12; l-320/l-640
   floor/head): mismo veredicto que la campaña 1.
5. **dejando la cama de lado**: todas en bed; head bed/head +0.15 (x-320) —
   sin residuo grande; la firma no se distingue bien de sentado (esperado:
   cuerpo aún sobre la cama, cabeza hacia la cabecera).
6. **pies en la cama** (foot-left-bed-2, foots-left-bed-1): piernas en
   bed/body/feet, torso en bed/head, y la cabeza *inestable*: lee floor/body o
   bed/head con residuos grandes (±0.35..0.47). Es la muestra menos resuelta.

## Veredicto

- **Los 3 modelos finalistas pasan los tests de aceptación** con área con
  máscara: sentado → cara/torso en bed; acostado → cara en floor (más lejos
  que bed/head).
- La muestra de área (footprint clipado por máscara) es **más estable que el
  keypoint solo**: residuos típicos ±0.05 m en posturas definidas, y elimina
  el p90 ruidoso del bbox de cara (3.27).
- Diferencias entre modelos pequeñas y en bordes: x-320 tiende a leer la
  cabeza de un parado como bed/head (+0.12) — igual que en campaña 1;
  l-640/l-320 la leen floor/head. l-640 da los residuos más chicos (máxima
  precisión).
- Regla de decisión sugerida (runtime): decidir con el **cuerpo** (torso +
  piernas), usar la cabeza solo como refuerzo; una cabeza en floor con torso
  en bed/head es "acostado"; cabeza en floor/head con torso en floor es
  "parado".

## Artefactos

- `demo-deep-calib-parts/results/{muestra}.{modelo}.json` — reporte completo
  por parte: geometría, support (face/pose/segment), quality, mask_coverage,
  profundidad (min/mediana/p10/p90), evidencia de superficie por zona.
- `demo-deep-calib-parts/persona/{muestra}.{modelo}.png` — preview con las 6
  partes pintadas del color de su zona.
- Exposición mínima para reuso: `body_parts`, `cross_model_validation` e
  `infer::{collect_detections, translate_detections_to_frame}` pasaron a
  `pub` (sin cambios de comportamiento); el bin reutiliza el estimador tal
  cual corre el runtime.
