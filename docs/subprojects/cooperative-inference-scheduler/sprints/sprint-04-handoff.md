# Handoff de Sprint 4: Validacion Cruzada Face/Pose

**Fecha de preparacion:** 2026-08-12
**Estado:** implementacion inicial completada; corrida fisica y ajuste clinico
pendientes
**Ultimo commit cerrado:** `c902d69 feat(scheduler): implementar urgencias cooperativas`
**Proposito:** usar pose como segunda evidencia para validar una observacion de
face sin exponer keypoints, masks ni payloads de modelos a `mana-control`.

Este documento sigue siendo el punto de entrada de la proxima sesion. El codigo
base ya esta implementado; lo siguiente es validar el blueprint face/pose con
hardware, no introducir same-frame dinamico.

## 1. Puerta De Salida

La puerta de Sprint 4 es:

> una observacion de face incierta puede solicitar pose de forma cooperativa,
> recibir una segunda evidencia en un keyframe posterior y publicar al FSM una
> senal semantica con `valid`, `quality`, `frame_number` y edad, sin cruzar
> keypoints ni bloquear el lazo de control.

Debe quedar demostrado que:

- una face incierta produce como maximo una request urgente pendiente por
  `(model_key, reason)`;
- la request se ejecuta en el siguiente punto seguro, nunca dentro del mismo
  keyframe que la produjo;
- la pose se asocia al mismo actor/gate de la cascada, no a otra persona;
- una validacion positiva y una negativa llegan al FSM como valores distintos;
- una validacion ausente o vieja no se interpreta como `valid = false` fresca;
- el FSM puede declarar una alerta que exige validacion cruzada;
- el control sigue a cadencia fija y no recibe payloads crudos de percepcion.

## 2. Estado Actual Verificado

- Sprint 1, Sprint 2 y Sprint 3 estan implementados.
- `c902d69` contiene `InferenceRequest`, cola persistente/transitoria,
  congelamiento por keyframe, prioridad, TTL, consumo one-shot, expiracion,
  starvation y metricas de espera.
- `cargo test --workspace --release` paso despues de la implementacion:
  - `mana-control`: 163 tests;
  - `mana-geometry`: 72 tests;
  - `mana-lite`: 161 tests;
  - `mana-media`: 17 tests;
  - `mana-perception`: 34 tests;
  - integraciones y doctests verdes.
- `git diff --check` estaba limpio al cerrar Sprint 3.
- `cargo fmt --check` conserva diferencias de formato preexistentes del
  worktree, fuera del cambio funcional del scheduler.
 - El catalogo local contiene copias fisicas de 68 ONNX: 44 de
   `yolo26-fp16` (incluidas las variantes `192`), 18 de `yoloface-fp16` y 6 de
   `depth-fp16`. Los subdirectorios de `tools/model-tools/artifacts/` ya no
   dependen de symlinks y permanecen ignorados por Git por su tamano.
   `model-tools inspect` cargo correctamente las variantes FP16 `192` de pose,
   deteccion y face; pose tiene entrada `[1, 3, 192, 192]` y salida
   `[1, 300, 57]`. El workshop CPU ya se midio con `s/m` en `192/320` contra
   `clip1`; la evidencia esta en su README.
- El worktree conserva cambios previos de blueprints, modelos, workshop y
  documentacion. No hacer `reset`, `checkout` ni revertirlos.

## 3. Gaps Reales Del Runtime

### Percepcion

- `core/mana-perception/src/detection.rs:Detection` ya conserva
  `keypoints: Option<Vec<[f32; 3]>>`.
- `src/infer/run_helpers.rs` traduce los keypoints del crop al frame original.
  No volver a aplicar ese offset.
- `DetectionEvidence` y `ConsolidatedObservation` no conservan keypoints; hoy
  el consolidado conserva bbox, confianza, modelo y mask.
- `src/app/inference.rs:PendingModelOutput` conserva la salida completa durante
  el keyframe y es el punto mas seguro para leer pose antes de perderla.
- `PendingModelOutput` conserva tambien el `CascadeTarget` usado por la
  ejecucion para asociar la evidencia al mismo track.
- `src/app/face_pose.rs` valida la geometria y `inference.rs` produce la request
  transitoria, consume la evidencia posterior y proyecta solo el resultado
  semantico.

### Puerto De Control

- `core/mana-control/src/lib.rs:SceneSample` contiene ahora
  `face_pose_validation: Option<FacePoseValidation>`.
- `SceneObservation` contiene face como bbox/confianza, deliberadamente sin
  keypoints ni masks.
- `AgedEvidence<SceneSample>.observed_at` y
  `ProcessImage::observations_age_ms()` ya son el timestamp y la edad del
  resultado que consume el control. No agregar un `Instant` crudo dentro del
  payload semantico salvo que una prueba demuestre que el timestamp del sample
  no alcanza.

### Senales Y FSM

- `core/mana-control/src/signals/catalog.rs` declara once tags v1, incluyendo
  `cara.pose_validada` y `cara.pose_calidad` con ausencia explicita.
- `core/mana-control/src/scan.rs:update_signals()` produce las senales nuevas
  solo desde evidencia fresca.
- `core/mana-control/src/fsm/engine.rs` ya evalua guards sobre el snapshot de
  senales y mantiene el latch de face.
- `core/mana-control/src/fsm/tests/face_pose.rs` demuestra una alerta que exige
  face presente, pose validada y calidad minima; el FSM clinico de produccion
  aun no cambia de estado por pose hasta fijar esa politica.

## 4. Contrato Normativo

El resultado estrecho que cruza a `mana-control` debe ser equivalente a:

```rust
pub struct FacePoseValidation {
    pub valid: bool,
    pub quality: f32,
    pub frame_number: u64,
}
```

Reglas:

- `quality` siempre es finita y esta en `[0.0, 1.0]`.
- `valid = true` requiere que todos los gates de asociacion y geometria pasen.
- `valid = false` dentro de `Some(...)` significa que la validacion se ejecuto
  y rechazo la evidencia.
- `None` significa que no hubo resultado de validacion en ese sample; no debe
  transformarse silenciosamente en `false`.
- `frame_number` es el keyframe de la pose utilizada para validar.
- El timestamp de observacion es el `observed_at` del `AgedEvidence` que
  contiene el `SceneSample`.
- La edad que ve control es `ProcessImage::observations_age_ms()` y debe ser la
  que gobierne frescura/estado seguro.
- El payload no contiene keypoints, masks, indices de joints, nombres de
  modelos ni bbox internos usados para la geometria.

La forma recomendada es agregar
`face_pose_validation: Option<FacePoseValidation>` a `SceneSample` y dejar la
edad en el wrapper existente. Si la semantica exige distinguir varios
resultados por sample, usar una lista acotada y documentar el limite; no crear
una cola de payloads hacia control.

## 5. Validador Determinista

Crear el validador en el adaptador de aplicacion, recomendado:
`src/app/face_pose.rs`. No poner la politica clinica en `mana-control` ni en el
crate T1 de deteccion.

Entrada minima interna, que nunca cruza el puerto:

```text
face bbox + face confidence
pose bbox + pose keypoints [x, y, confidence]
person/track identity or cascade target
frame dimensions
validation thresholds
```

El primer corte debe:

1. confirmar el mapa de joints del ONNX real antes de fijar indices; para el
   perfil COCO esperado, verificar nose, eyes y ears contra el artefacto y
   escribir esa correspondencia en un test;
2. descartar keypoints no finitos o bajo `keypoint_min_confidence`;
3. exigir suficientes joints de cabeza para evaluar geometria;
4. comprobar que la region de cabeza/centro de joints es compatible con el bbox
   de face y con el bbox del mismo person track;
5. combinar confianza de face, confianza de joints y consistencia geometrica en
   un `quality` reproducible;
6. devolver `valid = false` con calidad finita cuando hubo evidencia suficiente
   pero la geometria fallo;
7. devolver ausencia o resultado no disponible cuando no hay joints suficientes,
   el actor no se puede asociar o la evidencia ya expiro, segun el contrato que
   fije el test.

No asumir que el orden de `Vec<[f32; 3]>` es un contrato clinico sin verificar
el modelo. No usar una distancia magica sin expresarla en proporcion del bbox o
en un knob de configuracion.

## 6. Origen De La Request

Hay dos bordes posibles y la proxima sesion debe elegir uno antes de cablear el
productor:

### Recomendado: productor en percepcion

Cuando percepcion termina de evaluar una face incierta, conserva un contexto
interno acotado y llama a `enqueue_transient_request()` para
`pose-standard`. La request queda en `VecDeque` y, por el congelamiento del
scheduler, solo puede correr en el siguiente keyframe. El FSM consume luego
`cara.pose_validada` y `cara.pose_calidad`.

Ventajas:

- la fuente de incertidumbre y los datos crudos viven en el mismo tier;
- no se pierde una request transitoria en `Slot<ControlDirective>`;
- no se agrega una espera ni un canal al lazo de control;
- el control recibe solamente el resultado semantico.

### Alternativa: FSM/control como productor

Si el requisito funcional exige que el FSM sea literalmente quien solicite la
validacion, usar `ControlDirective.urgent_requests` como estado persistente y
generar la request desde una politica explicita del control. En ese caso hay
que agregar:

- una fuente declarada de la politica y su umbral de incertidumbre;
- tests de reemplazo latest-wins y no reanimacion one-shot;
- una razon estable como `face-uncertain`;
- una prueba de que la directiva siguiente vacia limpia la urgencia.

No mezclar ambos productores para la misma razon. La primera implementacion debe
tener un unico dueño de la request para que las metricas no cuenten dos veces.

## 7. Contexto Y Frescura

Al producir la request, guardar en `PerceptionStage` un contexto minimo:

```text
face_bbox
face_confidence
track_id o identidad de la compuerta
source_frame_number
requested_at
expires_at
```

Reglas del contexto:

- solo se conserva un contexto pendiente por actor/razon;
- la request usa el TTL del scheduler y no un timer/worker adicional;
- pose debe asociarse al mismo track o al target de la regla, nunca al mayor
  bbox global por conveniencia;
- si expira la request, se descarta el contexto y no se publica validacion;
- si llega pose sin contexto, no se inventa una validacion;
- si llega una nueva face incierta para el mismo actor, deduplicar o reemplazar
  de acuerdo con una prueba determinista, sin acumular backlog;
- no implementar same-frame dinamico: la evidencia solicitada nace para el
  siguiente keyframe.

## 8. Senales Para El FSM

Agregar al catalogo estatico, con nombres ASCII y semantica de ausencia:

```text
cara.pose_validada  -> Bool, presente solo cuando hubo validacion
cara.pose_calidad   -> Ratio, presente solo cuando hubo validacion
```

El nombre final debe conservar la convencion `dominio.atributo` y actualizar
los tests de version/tamano del catalogo. `update_signals()` debe:

- insertar ambos valores cuando `face_pose_validation` es `Some` y la evidencia
  de observaciones es fresca;
- insertar `cara.pose_validada = valid` aunque sea `false` cuando la validacion
  se ejecuto y fallo;
- dejar ambos ausentes si no hubo resultado o si la evidencia esta vencida;
- construir `Ratio` con validacion de finitud/rango y no hacer `as` silencioso.

Agregar al menos un fixture FSM que transicione a una alerta cuando:

```text
cara.presente == true
cara.pose_validada == true
cara.pose_calidad >= umbral
```

y pruebas negativas para validacion ausente, `valid = false`, calidad baja y
evidencia stale.

## 9. Archivos Exactos De Integracion

### Adaptador de percepcion

- `src/app/face_pose.rs`: contrato interno y validador puro recomendado.
- `src/app/perception.rs`: contexto pendiente y productor de request; no
  bloquear ni crear workers.
- `src/app/inference.rs`: recoger face/pose desde `PendingModelOutput`, ejecutar
  el validador, completar `ClinicalSample` y proyectar el resultado.
- `src/app/record.rs`: solo si se agrega un evento de auditoria de validacion;
  no duplicar keypoints en el evento de control.
- `core/mana-perception/src/detection.rs`: solo tocar si se necesita conservar
  keypoints en evidencia interna; preferir no ampliar el consolidado si el
  validador puede leer `PendingModelOutput`.

### Puerto y control

- `core/mana-control/src/lib.rs`: `FacePoseValidation` y el campo de
  `SceneSample`.
- `core/mana-control/src/signals/catalog.rs`: tags y presencia.
- `core/mana-control/src/scan.rs`: produccion de tags desde el sample fresco.
- `core/mana-control/src/fsm` y sus fixtures: guards/transiciones y compilacion
  de catalogos.
- `src/app/mod.rs`: tocar solo si se elige el productor control/FSM para la
  request persistente.

### Observabilidad

- `src/logger/event/mod.rs` y constructores/serializadores: solo un evento
  compacto si la auditoria necesita registrar `valid`, `quality`,
  `frame_number` y edad.
- `src/metrics/mod.rs`: agregar contadores de validaciones intentadas,
  positivas, negativas, no disponibles y stale solo si una decision operacional
  los necesita; no reutilizar `urgent` para semantic validation.
- `docs/observability.md`: documentar los campos nuevos si se agregan.

### Pruebas y fixtures

- `core/mana-control/src/signals/catalog.rs` tests: catalogo y nombres.
- `core/mana-control/src/scan.rs` tests: presencia/ausencia y frescura.
- `src/app/face_pose.rs` tests: geometria y calidad.
- `src/app/tests.rs`: adaptacion a `SceneSample` y validacion stale.
- `tests/golden_synthetic_cycle.rs` y un nuevo golden cross-validation si hace
  falta: compatibilidad sin validacion y alerta validada.
- `src/config/mod.rs` o tests de catalogos: FSM/blueprint con la nueva senal.

## 10. Plan De Implementacion

### Paso 1: congelar el contrato (completado)

- Confirmar productor elegido: percepcion recomendado, control/FSM alternativo.
- Confirmar mapa de keypoints del modelo real.
- Fijar thresholds y TTL en una estructura testeable, no constantes dispersas.
- Fijar la semantica de `None`, `Some(valid=false)` y stale.

### Paso 2: validador puro (completado)

- Crear la entrada interna y `validate_face_pose()`.
- Probar positivo, geometria incompatible, joints insuficientes, no finitos,
  confianza baja y dos personas.
- Probar que todas las coordenadas estan en frame original, incluyendo crop.

### Paso 3: request y contexto (completado)

- Detectar la condicion de face incierta en percepcion.
- Guardar contexto con TTL y emitir una request transitoria estable.
- Verificar que nace despues del freeze y se atiende desde el siguiente keyframe.
- Verificar consumo one-shot, expiracion y ausencia de backlog.

### Paso 4: resultado semantico (completado)

- Ejecutar pose urgente con sus gates existentes.
- Asociar la salida al mismo actor.
- Crear `FacePoseValidation` sin mover keypoints al control.
- Proyectar `frame_number` y dejar timestamp/edad en `AgedEvidence`.

### Paso 5: senales y FSM (fixture completado)

- Agregar tags al catalogo y productores en `update_signals()`.
- Agregar guard/fixture de alerta que exija validacion positiva y calidad.
- Probar que ausente, falso, bajo umbral y stale no disparan la alerta.

### Paso 6: acceptance y cierre (pendiente fisico)

- Mantener los goldens sin validacion identicos.
- Agregar golden sintetico de face incierta -> pose -> validacion -> alerta.
- Ejecutar `cargo test --workspace --release` despues del cierre documental.
- Revisar `git diff --check`, enlaces y diff completo.
- Actualizar roadmap, memoria, README y crear el commit de Sprint 4.

## 11. Criterios De Aceptacion

- La request de pose nace solo ante la condicion de face incierta definida por
  configuracion.
- Una request producida durante un keyframe no se ejecuta en ese mismo keyframe.
- Pose no se ejecuta si falla el gate de parent, tracking, clase, count, ROI o
  crop, aunque la request sea urgente.
- La salida de pose se asocia al mismo actor que la face.
- El validador produce resultados deterministas para los fixtures fijados.
- `FacePoseValidation` lleva `valid`, `quality` y `frame_number`; el sample
  lleva timestamp y el control puede calcular edad.
- `cara.pose_validada` y `cara.pose_calidad` aparecen solo con evidencia fresca.
- Una validacion falsa no se confunde con ausencia de validacion.
- El FSM puede exigir validacion positiva y calidad minima en una alerta.
- Ningun keypoint, mask, indice de joint o tipo interno de percepcion cruza a
  `mana-control`.
- Sin validacion cruzada, los goldens y la politica actual permanecen iguales.
- El lazo de control no adquiere locks ni espera al hilo de percepcion.
- `cargo test --workspace --release` permanece verde.

## 12. Lo Que No Hacer

- No mover `Vec<[f32; 3]>` a `SceneObservation`.
- No agregar una dependencia de `mana-control` a `mana-perception`.
- No llamar pose directamente desde el detector face; publicar una request y
  dejar que el scheduler decida.
- No usar `Slot<ControlDirective>` para la request transitoria recomendada.
- No hacer same-frame dinamico ni preempcion en este sprint.
- No comparar face y pose de actores distintos por proximidad global.
- No usar `frame_number` sin edad ni reutilizar una validacion vencida.
- No convertir ausencia de resultado en `valid = false` sin declararlo en el
  contrato.
- No agregar un worker, timer o canal bloqueante para expirar contextos.
- No usar el workshop fisico como sustituto de fixtures deterministas.
- No revertir cambios previos del worktree.

## 13. Primer Comando De La Proxima Sesion

Desde la raiz del repositorio:

```sh
git status --short
cargo test --workspace --release
```

Luego leer solamente:

```text
docs/subprojects/cooperative-inference-scheduler/sprints/sprint-04-handoff.md
docs/subprojects/cooperative-inference-scheduler/spec.md secciones 5, 8 y 11
core/mana-perception/src/detection.rs: Detection, DetectionEvidence, ConsolidatedObservation
src/app/inference.rs: PendingModelOutput, ClinicalSample, project_scene_sample
src/app/perception.rs: PerceptionStage, enqueue_transient_request
core/mana-control/src/lib.rs: SceneSample, SceneObservation, AgedEvidence
core/mana-control/src/scan.rs: update_signals, evaluate_fsm
core/mana-control/src/signals/catalog.rs: scene_signal_catalog
core/mana-control/src/fsm/engine.rs: evaluate_snapshot_at, signal guards
```

La siguiente sesion debe comenzar con el blueprint de capacidad `s/192` o
`s/320`, una fuente RTSP reproducible y una ventana de 30 a 60 segundos. El
contrato y los fixtures ya estan cubiertos; no comenzar por same-frame dinamico.
