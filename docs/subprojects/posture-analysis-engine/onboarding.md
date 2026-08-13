# Onboarding: Sprint 5 de Analisis de Postura

**Proposito:** alinear contexto, lenguaje, limites y primer corte tecnico antes
de implementar.
**Estado:** propuesta de mapa operativo para alinear antes de comenzar.
**Modelo inicial:** `depth-l-640`.
**Entrada inicial:** reportes JSON offline, una persona por imagen.

## 1. La mision

El sprint no intenta crear otro modelo de vision. Intenta convertir evidencia ya
calculada en una decision de postura explicable:

```text
JSON de percepcion
        + TOML maestro de superficies
        + perfiles TOML por postura
        v
adaptador normalizado
        v
features y calidad por componente
        v
consenso por postura
        v
JSON explicable
```

La salida es diagnostica. En este sprint no cambia el FSM, no llega a
`mana-control` y no gobierna una accion clinica.

## 2. Orden de autoridad

Cuando dos documentos parezcan diferir, se usa este orden:

1. `spec.md`: contrato normativo de entradas, estados, scoring y salida.
2. `adrs/001..004`: decisiones de arquitectura ya tomadas.
3. `sprints/sprint-05-posture-signature-engine.md`: entregables y acceptance.
4. `technical-memory.md`: evidencia empirica y semillas de firmas.
5. `demo-deep-calib-*` y `scripts/build-posture-signatures.py`: artefactos de
   experimentacion reproducible.

La memoria y los JSON describen lo observado. No convierten una muestra en un
umbral clinico ni autorizan autoaprendizaje.

## 3. Punto de partida verificado

### Contexto de profundidad

- Modelo: `depth-l-640`.
- Directorio de resultados: `l-640`.
- ROI: `[452, 140, 1300, 1029]`.
- Semantica: profundidad relativa del modelo en la misma camara, ROI y modelo.
- Baseline: cuatro frames vacios; variacion maxima observada aproximada `0.021`.
- Zonas: `bed/head`, `bed/body`, `bed/feet`, `floor/head`, `floor/body`,
  `floor/feet`.

`depth_m` es un nombre historico de los reportes. El engine lo trata como valor
de modelo relativo, no como metros fisicos.

### Reportes disponibles

- `demo-deep-calib-radio/l-640/<sample>.json`: bbox de persona, segmentacion,
  face, keypoints con coordenadas de frame, profundidad puntual y zonas.
- `demo-deep-calib-parts/results/<sample>.l-640.json`: partes corporales,
  calidad, cobertura de mascara, profundidad por area y evidencia de superficie.
- `demo-deep-calib-parts/signatures-l-640.json`: matriz descriptiva generada,
  util como semilla y auditoria, no como perfil normativo final.
- `scripts/build-posture-signatures.py`: transformacion reproducible de ambos
  reportes.

### Tipos existentes

- `core/mana-perception/src/surface_calibration.rs`: `SurfaceCalibration`,
  `SurfaceZone` y validacion del contexto de superficie.
- `src/app/body_parts.rs`: `BodyPartsEstimate`, `BodyPartEstimate` y
  `BodyPartDepth` con calidad, cobertura, profundidad y frescura.
- `src/bin/deep-calib-radio.rs`: serializacion del reporte de radio.
- `src/bin/deep-calib-parts.rs`: ejecucion diagnostica de
  `BodyPartsEstimator`.

Los DTOs de los bins son privados y `BodyPartsEstimate` es un tipo de runtime.
El engine offline debe consumir el contrato serializado, no importar detalles
internos de los bins ni depender del lazo de percepcion.

## 4. Lenguaje comun

### Observacion

Un valor detectado en una captura: una bbox, un grupo de joints, una parte o una
mediana de profundidad. Puede estar `observed`, `partial`, `missing`, `invalid`,
`stale` o `conflict`.

### Feature

Una relacion que puede compararse con un perfil, por ejemplo
`geometry.torso_tilt_deg` o `body_part.left_leg.relative_to_torso`.

### Componente

Una familia de evidencia: `geometry`, `body_parts`, `depth`, `face` o `segment`.
Cada componente tiene score, calidad, peso efectivo y razones.

### Perfil

Un TOML por postura con centros, tolerancias, pesos, features y quorum. Es una
referencia versionable y de solo lectura durante el analisis.

### Candidato

El resultado de comparar una captura contra un perfil. Conserva el score y el
detalle de por que gano, perdio o quedo incompleto.

### Decision

La salida global: `classified`, `ambiguous`, `unknown` o `incompatible` cuando
el contexto de modelo, ROI o frame no permite comparar.

## 5. Modelo mental de senales

La prioridad del primer corte es:

```text
geometria 2D y partes con mascara
        > profundidad relativa
        > face como refuerzo de cabeza
```

Reglas que no se deben invertir durante la implementacion:

- Un area corporal con buena cobertura pesa mas que un keypoint aislado.
- Face aporta a `head`; no decide por si sola entre sentado, acostado y parado.
- La ausencia de face o de una pierna no invalida al actor completo.
- `mask_coverage` bajo reduce peso y calidad; no se arregla inventando pixeles.
- `in_envelope` es evidencia auditable, no un gate universal de postura.
- La profundidad de `depth-person` no se compara contra `SurfaceCalibration`.
- Un valor faltante no es un conflicto.
- Dos valores observados incompatibles si son un conflicto explicito.
- El margen entre los dos mejores candidatos importa tanto como el score mayor.

## 6. Frontera de implementacion

El primer corte recomendado queda separado en dos capas:

```text
src/posture_analysis.rs
    perfiles, observaciones normalizadas, features, scoring y salida

src/bin/posture-analysis.rs
    argumentos, lectura de JSON/TOML, pairing de reportes y escritura de JSON
```

La biblioteca no ejecuta modelos ni conoce RTSP, scheduler, slots, FSM o
`mana-control`. El bin solo coordina archivos y llama funciones puras.

No se modifican durante este sprint:

- `SurfaceCalibration` ni `depth-rules.toml`.
- La forma de `Detection`, `ConsolidatedObservation` o `SceneSample`.
- El pipeline de inferencia y sus cadencias.
- El contrato del FSM.
- Las mascaras, keypoints o geometrias originales.

## 7. Primer corte de codigo

El orden tecnico es intencional:

1. Definir DTOs serde para `master.toml` y perfiles.
2. Validar schema, ids unicos, paths, modelo, ROI, tolerancias y pesos.
3. Definir una observacion normalizada independiente del JSON de origen.
4. Implementar el adaptador de radio y parts para una persona.
5. Implementar soporte numerico, categorico y de presencia.
6. Emitir el candidato explicable para una sola captura.
7. Agregar el consenso entre los siete perfiles.
8. Crear fixtures parciales y casos de contexto incompatible.

No comenzar por pesos clinicos, smoothing temporal ni integracion runtime.

## 8. Contratos operativos

### Maestro

El archivo sera:

```text
config/posture-analysis/l-640/master.toml
```

Debe fijar `model_key`, semantica de depth, ROI o referencia a la calibracion,
version de schema, politicas de quorum y la lista cerrada de perfiles.

### Perfiles

Los siete perfiles viviran junto al maestro. Sus valores seran provisionales y
blandos. Se pueden editar explicitamente y versionar; una captura analizada no
los actualiza.

### Pairing offline

Una imagen se analiza con el par de reportes del mismo sample y contexto:

```text
demo-deep-calib-radio/l-640/acostado-1.json
demo-deep-calib-parts/results/acostado-1.l-640.json
```

Si falta un reporte, el adaptador debe producir evidencia faltante o un error
de entrada explicito segun el componente, nunca leer silenciosamente otro
modelo o tamano.

## 9. Pruebas que nos alinean

### Unitarias

- parser y validacion de master y perfiles;
- soporte triangular numerico;
- categorias permitidas y categorias observadas incompatibles;
- quorum y renormalizacion de pesos;
- margen de ambiguedad;
- estados `missing`, `partial`, `invalid`, `stale` y `conflict`.

### Golden offline

- siete muestras positivas con el mismo contexto `depth-l-640`;
- face ausente;
- un joint de pierna ausente;
- mascara parcial o cobertura cero en una parte;
- keypoint outlier frente a area corporal consistente;
- dos candidatos dentro del margen: `ambiguous`;
- quorum insuficiente: `unknown`;
- modelo, ROI o dimensiones incompatibles: `incompatible`.

### Propiedades

- misma entrada produce el mismo JSON semantico;
- reordenar reportes no cambia la decision;
- eliminar una fuente no inventa conflicto;
- una parte parcial no borra el lado visible;
- el engine no escribe perfiles ni calibraciones.

## 10. Definicion de listo

El onboarding queda alineado cuando aceptamos estas cinco afirmaciones:

1. Sprint 5 es offline y por imagen; no es todavia una politica clinica.
2. Los JSON de `deep-calib-radio` y `deep-calib-parts` son la entrada del primer
   corte.
3. La biblioteca del engine queda aislada del runtime y el bin solo hace IO.
4. Las firmas iniciales son provisionales, blandas y sin autoaprendizaje.
5. `unknown` y `ambiguous` son resultados correctos, no errores a ocultar.

La puerta de salida del sprint sigue siendo la del sprint normativo: las siete
muestras deben producir JSON explicable y determinista, con evidencia parcial y
contexto incompatible reportados de forma explicita.

## 11. Protocolo de trabajo en este repositorio

- El worktree ya contiene cambios previos de codigo, configuracion, fixtures y
  diagnosticos. No usar `reset`, `checkout` ni revertir cambios ajenos.
- Revisar `git status --short` antes de editar y separar cambios del engine de
  los artefactos previos.
- Ejecutar tests focalizados durante el desarrollo y la suite workspace antes
  de cerrar el sprint.
- Ejecutar `git diff --check` y revisar el diff completo al finalizar.
- No crear commit hasta que se solicite expresamente.

## 12. Primer paso despues del onboarding

Implementar el parser y validador de `master.toml` y perfiles, junto con tests
de schema y contexto. La primera ejecucion no debe clasificar posturas: debe
demostrar que los perfiles se cargan, se validan y se rechazan cuando el modelo
o la ROI no coinciden.
