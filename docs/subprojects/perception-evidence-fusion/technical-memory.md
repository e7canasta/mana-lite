# Memoria tecnica: Fusion de Evidencias y Estimador de Partes Corporales

**Estado:** validacion cruzada, body parts, depth diagnostico y calibracion de
superficies implementados; temporalidad completa y postura clinica fuera de
alcance
**Ultima actualizacion:** 2026-08-13

## 1. Problema

La cascada puede producir varias evidencias del mismo actor con distinta
calidad, frecuencia y frescura:

```text
detect  -> persona y bbox
face    -> bbox de cara
pose    -> bbox, keypoints y confianza por joint
segment -> bbox, mascara y confianza de mascara
```

La consolidacion espacial evita duplicados, pero no expresa por si sola:

- si face y pose describen la misma cabeza;
- si los keypoints caen en una mascara razonable;
- si una mascara cubre mucho mas cuerpo del que la pose sugiere;
- si una contradiccion es puntual o persiste durante varios frames;
- como estimar partes cuando una fuente llega tarde o falta.

No conviene resolver esas dos preguntas con el mismo objeto. Validar evidencias y
construir geometria son operaciones diferentes y tienen distintos consumidores.

## 2. Estado real del runtime

El punto de integracion disponible hoy es `PendingModelOutput` en
`src/app/inference.rs`. Durante un keyframe conserva `model_key`,
`InferenceResult`, `CascadeTarget` y el contexto del crop. Es la ultima etapa en
la que coexisten modelo, keypoints, mascaras y target.

La consolidacion posterior es stateless y pierde parte de esa informacion:
`DetectionEvidence` y `ConsolidatedObservation` no conservan keypoints, frame por
fuente ni frescura independiente. Por eso el assembler de evidencia debe correr
antes de `DetectionConsolidator::consolidate`, dentro del hilo de percepcion.

La identidad es parcial. Un `CascadeTarget.id` puede representar un track
confirmado para un child; sin target solo existe evidencia frame-local. El
runtime actual no tiene un `ActorRef`, un store temporal por fuente ni soporte de
targets multiples para todos los actores. Esas son entregas del proyecto, no
supuestos existentes.

El camino especializado face/pose ya existe en `src/app/face_pose.rs`: una face
incierta puede generar una request urgente, pose se ejecuta en un keyframe
posterior y control recibe `FacePoseValidation`. El validador generico debe
reutilizar su geometria sin crear una segunda request ni romper la semantica de
`None` frente a `Some(valid = false)`.

El blueprint por defecto `detect-room-face` no habilita pose ni segmentacion. Las
corridas del MVP deben seleccionar un blueprint con las cuatro fuentes, como
`config/blueprints/detect-face-pose-seg/blueprint.toml`.

La implementacion inicial de validacion vive en
`src/app/cross_model_validation.rs`. Se invoca desde `run_inference` con la lista
completa de `PendingModelOutput` y publica un evento debug compacto. Solo acepta
targets con `CascadeTarget.id`; no conserva evidencia entre keyframes y reporta
`freshness = 1.0` por tratarse de una comparacion del frame actual.

## 3. Dos responsabilidades

### 2.1 Cross-model validation

Recibe evidencias asociadas al mismo actor y produce una lectura de consistencia.
Puede combinar:

- confianza de cada modelo;
- compatibilidad de bbox y containment;
- distancia normalizada entre face y joints de cabeza;
- proporcion de keypoints validos dentro de la mascara;
- exceso de mascara fuera del soporte de pose;
- frescura y estabilidad en `t-n ... t`.

Su salida es un score y un conjunto de razones. No crea una nueva deteccion ni
debe ser un veto binario por defecto.

### 2.2 Body parts estimator

Recibe las evidencias disponibles y construye geometria derivada por partes. Las
partes iniciales son:

```text
head, torso, left_arm, right_arm, left_leg, right_leg
```

`hands` y `feet` pueden agregarse cuando la resolucion y el modelo lo permitan.
El estimador usa pose como estructura, face como ancla localizada y segmentacion
como evidencia de cobertura y borde. Puede devolver una parte con calidad baja o
ausente; no debe inventar precision donde no existe evidencia.

## 4. Confianza y colaboracion

La confianza del modelo y la consistencia entre modelos son dimensiones
distintas. Para una evidencia `i` se mantienen al menos:

```text
source_confidence  -> confianza declarada por el modelo
agreement_quality  -> acuerdo con otras evidencias
freshness_quality  -> calidad por edad del frame
temporal_quality   -> estabilidad en la ventana
```

Una confianza fusionada puede usar esas dimensiones, pero la formula exacta debe
calibrarse con fixtures y corridas. El modelo con mejor confianza ponderada puede
ser el ancla de una parte; las otras fuentes siguen aportando corroboracion,
expansion o penalizacion.

No se debe interpretar "ancla" como "autoridad". Una mascara grande y confiada
puede ser corregida por pose estable; una pose parcial puede ser completada por
segmentacion sin convertir sus keypoints ausentes en observaciones visibles.

## 5. Geometria de partes

El soporte de pose debe construirse desde segmentos ensanchados, no desde un
convex hull global del cuerpo. El convex hull mezcla brazos y piernas separados
y hace parecer incorrecta una mascara que en realidad es razonable.

La primera geometria derivada puede seguir este orden:

1. Crear segmentos entre joints relacionados.
2. Ensanchar cada segmento con un radio proporcional al bbox de la persona.
3. Construir regiones iniciales de cabeza, tronco, brazos y piernas.
4. Usar la mascara para recortar o refinar el borde de cada region.
5. Asignar calidad por parte, conservando las fuentes y sus frames.
6. Suavizar la salida con el estado temporal, sin modificar la mascara cruda.

La relacion entre pose y segmentacion es suave:

- inclusion de joints en la mascara aumenta la calidad;
- joints cerca del borde reducen la calidad gradualmente;
- mascara muy alejada del soporte de pose reduce la calidad;
- oclusion, joints no visibles y mascara de baja calidad reducen el peso de la
  evidencia en vez de invalidar toda la persona.

La mascara de `DetectionMask` tiene representaciones distintas: `polygons` se
normaliza al frame, mientras `compact` permanece en el espacio de mascara y usa
`origin` y `mask_dims`. El estimador debe consultar el espacio correcto y probar
explicitamente crops con offset.

El mapa real de keypoints es `Vec<[f32; 3]>`; no existe un cuarto valor de
visibilidad. La constante actual de cabeza cubre los joints COCO esperados, pero
el mapa completo requerido por brazos y piernas debe fijarse en un adaptador y
validarse contra el artefacto pose.

## 6. Tiempo

Las evidencias se alinean por `CascadeTarget.id` cuando es inequívoco y por
`frame_number` dentro del keyframe. Un resultado sin target es `FrameLocal`, no
una identidad temporal. Nunca se deben comparar face y pose de actores distintos
solo por proximidad global.

La ventana `t-n ... t` sirve para:

- estabilizar keypoints y poligonos;
- amortiguar una contradiccion aislada;
- degradar evidencia envejecida;
- conservar una parte estimada cuando un modelo no corre en ese frame.

Cada evidencia conserva su edad. La salida temporal no puede ocultar que una
parte proviene de un frame anterior.

## 7. Fronteras de runtime

```text
CascadeScheduler
    decide cuando ejecutar modelos

EvidenceAssembler / association
    decide que salidas del keyframe pueden describir al mismo actor

DetectionConsolidator
    consolida bbox y componentes sin memoria temporal

CrossModelValidator
    mide acuerdo y contradiccion

BodyPartsEstimator
    construye geometria derivada

mana-control
    recibe solo resumen semantico estrecho y envejecido
```

El estimador y el validador viven en percepcion/adaptadores de aplicacion. El
control no recibe keypoints, mascaras ni poligonos internos salvo que una futura
decision de producto defina un contrato semantico pequeño. El store temporal
futuro sera propiedad de `PerceptionStage`, sin `Mutex` ni ownership en
`mana-control`.

## 8. Riesgos y preguntas abiertas

- La mascara de segmentacion puede ser deliberadamente mas amplia que la
  silueta visible por el entrenamiento del modelo.
- Los modelos de pose pueden producir joints plausibles fuera de una mascara
  parcial por oclusion.
- Face y pose pueden correr en frames distintos; el matching temporal necesita
  TTL y asociacion explicita.
- Las metricas de calidad requieren calibracion por tamano de modelo y resolucion.
- Un poligono derivado puede ser util para visualizacion sin ser apto para una
  decision clinica.
- El contrato de salida de body parts debe permanecer interno hasta probar que un
  consumidor real lo necesita.
- `Track.evidence` solo conserva nombres de modelos y `Track` suaviza bbox; no
  existe aun frescura por fuente ni historial de keypoints/partes.
- La validacion face/pose actual tiene `valid: bool` para el FSM aunque su
  `quality` sea continua; la validacion generica puede ser gradual sin cambiar
  ese contrato especializado en el primer corte.

## 9. Profundidad relativa a superficies

El objetivo operativo no es reconstruir una postura 3D general. La escena es
fija, la camara esta instalada en un angulo picado y los estados posibles estan
acotados a una cama, su borde y el piso. Por eso la referencia adecuada es una
envolvente de profundidad local por superficie y subzona.

La cama se calibra como parches semanticos, inicialmente `head`, `body` y
`feet`. El piso puede tener una zona unica o varias zonas `near`, `middle` y
`far` si la perspectiva produce demasiada dispersion. Cada parche conserva su
poligono global, mediana, percentiles, MAD, cobertura y fingerprint del modelo,
ROI y tamano de frame.

La referencia de una zona no es un unico valor global ni un plano 3D. Es un
baseline espacial observado con la misma camara y el mismo `depth-scene` que
usa el runtime. La postura consulta el residuo de cada parte respecto a la
envolvente de la zona, junto con su interseccion geometrica y su persistencia.

La profundidad del crop `depth-person` no se compara contra estas referencias.
Ese mapa es local a cada crop y solo aporta relaciones internas entre partes.

## 10. Calibrador aislado

`deep-calib` es un binario auxiliar independiente del comando `mana-lite`. No
levanta `App`, no arranca el FSM y no modifica el ciclo de percepcion. Importa
el cargador de modelo, `DepthFrame` y `polygon_stats` a traves de la libreria,
pero su sesion y su escritura de archivos viven en el propio calibrador.

La sesion se guarda en `deep-calib.toml`, se actualiza de forma atomica y puede
reanudar zonas ya marcadas. La promocion hacia configuracion de runtime es una
operacion explicita. La redireccion de stdout no se usa como mecanismo de
persistencia.

La primera interfaz acepta una imagen o frame reproducible y poligonos
explicitos; la captura interactiva puede agregarse encima de ese contrato sin
mezclar UI con estadistica. La referencia debe invalidarse si cambia el modelo,
su digest, el ROI, la resolucion o la camara.

## 11. Decisiones que no tomamos

- No convertir depth monocular en coordenadas metricas sin una referencia
  fisica comprobable.
- No ajustar intrinsecos, extrinsecos ni un plano 3D para resolver este caso
  acotado.
- No usar `min/max` como envolvente principal; los outliers se absorben con
  percentiles y MAD.
- No comparar valores absolutos de crops `depth-person` distintos.
- No promover automaticamente una calibracion al FSM ni a
  `depth-rules.toml`.
- No inferir postura solo desde la coordenada vertical de la imagen o el signo
  supuesto del mapa depth.

## 12. Estado destilado

La implementacion que debe considerarse vigente es:

```text
PendingModelOutput
    -> CrossModelValidator
    -> BodyPartsEstimator
    -> depth por huella corporal
    -> SurfaceCalibration / deep-calib
    -> diagnostico JSONL/Rerun
```

La validacion cruzada y el estimador consumen evidencia del mismo keyframe y no
ejecutan modelos entre si. El estimador puede producir partes parciales y
calidad por parte; no modifica la mascara cruda ni publica geometria interna a
`mana-control`.

`deep-calib` es un bin auxiliar aislado. La calibracion es especifica de modelo,
ROI, resolucion y camara. `depth-person` solo aporta relaciones internas del
actor; la comparacion contra cama y piso usa `depth-scene` y zonas globales.

La persistencia temporal acotada de geometria existe como modo opt-in, pero no
hay todavia un `EvidenceStore` completo por fuente ni una politica temporal
clinica. Esas son limites tecnicos, no tareas de un sprint activo.
