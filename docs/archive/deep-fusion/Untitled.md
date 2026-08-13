La idea es correcta, pero con estas correcciones importantes:
1. El polígono de segmentación no separa partes corporales
La segmentación nos da el contorno de la persona. Las partes deben construirse combinando:
- pose: keypoints y segmentos anatómicos.
- segmentation: máscara que limita cada parte al cuerpo real.
- Geometría: torso como cuadrilátero, brazos y piernas como cápsulas alrededor de los segmentos, cabeza como región local.
Así evitamos muestrear profundidad fuera de la persona.
2. La profundidad por parte debe ser estadística
No conviene asignar un único valor. Para cada parte deberíamos calcular:
depth_median
depth_p10
depth_p90
depth_spread
valid_pixels
valid_fraction
relative_to_torso
bed_plane_residual
Las manos y pies necesitarán más tolerancia porque ocupan pocos píxeles. La decisión debe usar la mediana y persistencia temporal, no un píxel aislado.
3. La profundidad monocular no necesariamente está en metros
El depth-standard actual produce un DepthFrame local al ROI, pero no debemos asumir automáticamente que sus valores son distancia métrica calibrada. Inicialmente conviene tratarla como profundidad relativa.
Para distinguir acostado, sentado o parado, lo correcto sería estimar el plano de la cama:
- Ajustar un plano usando la profundidad de la zona de la cama.
- Comparar cada parte corporal contra ese plano.
- Si existe calibración de cámara e intrínsecos, convertir a coordenadas 3D y calcular altura sobre la cama.
- Sin calibración, usar residuos relativos y clasificar con evidencia combinada de pose, máscara y depth.
Ejemplos:
- Acostado: torso, cabeza y extremidades permanecen cerca del plano de la cama.
- Sentado: cadera cerca de la cama, torso y cabeza elevados.
- Parado: pies cerca del plano, torso y cabeza claramente elevados.
- Extremidad fuera de la cama: la región sale de la zona de cama y mantiene un residuo de profundidad/altura distinto durante varios frames.
4. Sí conviene ejecutar depth sobre la bbox de la detección base
Pero no reemplazaría el depth global actual. Recomiendo dos ramas:
- depth-scene: ROI fija actual, para cama, entorno y plano de referencia.
- depth-person: modelo hijo de la detección/tracking, usando la bbox de la persona con margen de 15-25%.
El crop de persona mejora la resolución efectiva y permite usar modelos FP16 a 320. 192 puede servir para ahorrar recursos, pero será más débil para manos y pies. FP16 es precisión reducida, no cuantización INT8.
La profundidad del crop de persona sólo debería compararse internamente entre sus propias partes. No conviene comparar valores absolutos entre crops diferentes sin normalización o alineamiento.
Arquitectura propuesta
detect + track
       |
pose + segmentation
       |
depth-scene -----------------> plano de cama / contexto
       |
depth-person sobre bbox ------> profundidad relativa por parte
       |
body-part footprints
       |
DepthEvidence por parte
       |
clasificador temporal de postura
El primer cambio debería ser una API tipo polygon_stats en DepthFrame, que:
- Reciba un polígono en coordenadas globales.
- Lo transforme correctamente al ROI y a las dimensiones reales del mapa.
- Ignore valores inválidos.
- Devuelva estadísticas robustas y cobertura.
Después agregamos DepthEvidence a cada body part y lo publicamos inicialmente en JSONL/Rerun como diagnóstico. La clasificación de postura y “extremidad fuera de cama” debe venir después de calibrar umbrales con datos reales.
La decisión recomendada es: sí usar depth por bbox de persona, pero mantener el depth global para la cama y usar los polígonos corporales derivados de pose + máscara, no de la máscara sola.

[✓] Inspeccionar interfaces actuales de depth, body parts, cascade y eventos
[✓] Diseñar contrato de DepthEvidence y muestreo poligonal con coordenadas ROI
[✓] Implementar estadísticas depth por body part y asociación temporal
[✓] Integrar configuración, JSONL/Rerun y rama depth por bbox si el pipeline lo soporta
[✓] Añadir tests y documentación del sprint
[✓] Ejecutar fmt, tests y validaciones del repositorio

**Sprint implementado**

- `mana-perception::polygon_stats` para muestrear polígonos y cápsulas sobre `DepthFrame`.
- Conversión correcta entre coordenadas globales, ROI y resolución real del mapa.
- Recorte de cada body part contra los polígonos de segmentación.
- Estadísticas robustas: mediana, p10, p90, min/max, píxeles muestreados y válidos.
- `relative_to_torso_m` por parte corporal.
- Asociación preferente al `depth-person` del `track_id`, con fallback a `depth-standard`.
- Nuevo blueprint opt-in:
  ```text
  config/blueprints/detect-face-pose-seg-depth/blueprint.toml
  ```
- Modelos `depth-person-s-320` y `depth-person-s-192` con crop sobre bbox.
- Evidencia depth incluida en JSONL `body_parts` y escalas diagnósticas en Rerun.
- Depth de bbox aislado de las reglas clínicas de cama; sólo el ROI de escena actualiza el FSM.
- Documentación agregada en `sprint-03-depth-body-parts.md`.

**Verificación**

- `cargo test --features rerun`: **179 tests passed**.
- `cargo test -p mana-perception`: **36 tests passed**.
- `cargo fmt --all -- --check`: correcto.
- `git diff --check`: correcto.
- Clippy global sigue bloqueado por advertencias preexistentes del workspace.
- `--no-default-features` continúa fallando por la integración FFmpeg existente.

La clasificación clínica de acostado/sentado/parado y extremidad fuera de cama queda como siguiente capa, usando las referencias calibradas de cama/piso sobre esta evidencia ya integrada.
