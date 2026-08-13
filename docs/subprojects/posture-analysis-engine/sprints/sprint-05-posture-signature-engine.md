# Sprint 5: Engine Offline de Analisis de Postura por Firmas

**Estado:** listo para implementacion
**Dependencia:** `perception-evidence-fusion` Sprint 2, Sprint 3 y Sprint 4
**Modelo inicial:** `depth-l-640`
**Dataset inicial:** siete muestras de `samples`

## Objetivo

Crear un engine offline que reciba los resultados ya calculados por mana-lite y
produzca una matriz de candidatos de postura con explicacion por componente. El
engine debe tolerar face, keypoints o mascara parciales y debe preferir
`unknown`/`ambiguous` antes que inventar una postura.

## Entregables

### A. Artefactos TOML

- `config/posture-analysis/l-640/master.toml`.
- Un TOML por cada postura inicial.
- Referencia obligatoria a la sesion `SurfaceCalibration` maestra.
- Centros, tolerancias, pesos y quorum por feature.
- Version de esquema y contexto de modelo/ROI.

### B. Adaptador de observaciones

- Leer bbox de persona, face, keypoints y segmentacion.
- Leer geometria, calidad, cobertura y depth de `BodyPartsEstimator`.
- Normalizar coordenadas respecto al bbox de persona.
- Conservar estado `missing`, `partial`, `invalid`, `stale` o `conflict`.
- No volver a ejecutar inferencia.

### C. Scoring

- Score independiente para geometria, partes, depth y face.
- Pesos renormalizados sobre componentes observados.
- Penalizacion explicita de conflictos observados.
- Quorum minimo y margen de ambiguedad.
- Resultado `classified`, `ambiguous` o `unknown`.

### D. JSON explicable

- Contexto de calibracion y compatibilidad.
- Candidatos ordenados.
- Score por componente y feature.
- Observaciones usadas.
- Fuentes faltantes o parciales.
- Conflictos y razones legibles.
- Version exacta del perfil.

### E. Fixtures y replay de imagenes

- Las siete muestras actuales como fixtures positivas.
- Casos derivados con face ausente, joint ausente y mascara parcial.
- Repeticion determinista sobre el mismo JSON.
- Comparacion de resultados antes y despues de actualizar tolerancias.

## Orden de trabajo

1. Implementar el parser de `master.toml` y perfiles de postura.
2. Implementar el adaptador puro desde los reportes existentes.
3. Implementar soporte numerico, categorico y de presencia.
4. Implementar quorum, renormalizacion y margen de ambiguedad.
5. Emitir el JSON explicable.
6. Crear fixtures de entradas parciales.
7. Ejecutar las siete muestras y revisar manualmente las razones.
8. Ajustar perfiles solo mediante una nueva version TOML.

## Criterios de aceptacion

- `acostado-1` produce una firma donde cabeza/brazos se separan del torso y
  piernas permanecen cerca de cama.
- `sentado-1` produce una firma donde cabeza/hombros/brazos estan en cama y las
  piernas se desplazan hacia `bed/feet`.
- `sentado-borde-1` produce torso en cama y piernas parciales o mixtas.
- `parado-aside-1` produce torso y piernas en una banda comun de piso, aunque
  face o un joint individual discrepe.
- `leaving-bed-aside-head-1` conserva torso y piernas en cama con cabeza
  desplazada.
- `foot-left-bed-2` y `foots-left-bed-1` conservan la asimetria de cobertura y
  no son forzadas a `parado` por una sola extremidad.
- Si face falta, el analisis puede continuar con partes y geometria.
- Si una pierna falta, la otra participa y el JSON marca parcial.
- Si la mascara tiene baja cobertura, el score baja y aparece la razon.
- Si dos posturas quedan dentro del margen de ambiguedad, el resultado es
  `ambiguous`.
- Si no se cumple el quorum, el resultado es `unknown`.
- Modelo, fingerprint, ROI y frame incompatibles se rechazan explicitamente.
- El engine no modifica `SurfaceCalibration`, perfiles TOML, FSM ni
  `depth-rules.toml`.

## Fuera de alcance

- Ejecucion de modelos.
- Captura RTSP.
- Multiples actores.
- Ventana temporal completa.
- Autoaprendizaje de firmas.
- Activacion clinica.
- Coordenadas 3D metricas.

## Handoff para la proxima sesion

La implementacion debe comenzar leyendo este sprint junto con:

- `onboarding.md` para el mapa compartido y la frontera offline.
- `technical-memory.md` para el marco empirico.
- `spec.md` para el contrato.
- `adrs/001..004` para las decisiones que no deben reabrirse sin nueva
  evidencia.

El primer codigo del sprint debe ser el parser y validador de perfiles, no el
clasificador final ni la integracion al runtime.
