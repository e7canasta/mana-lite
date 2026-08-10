# Diseño técnico — Señales de escena

**Estado:** decidido para implementación
**Alcance:** estado objetivo al completar las etapas A-D
**Contrato normativo:** [1-spec.md](1-spec.md) define los tipos, operadores y evolución pública. Este documento decide cómo se implementa ese contrato en el engine actual.
**Documentos relacionados:** [2-sprints.md](2-sprints.md), [3-sprint-1.md](3-sprint-1.md), [4-big-picture.md](4-big-picture.md), [5-engine-funcional.md](5-engine-funcional.md) y [ADR-032](../adrs/032-scene-signals-as-contract.md).

## 1. Decisión

El sistema incorporará un catálogo estático de señales de escena, una tabla inmutable por ciclo de control y un guard genérico de FSM llamado `Signal`.

La solución conserva los motores especializados existentes para zonas, salud y profundidad. No crea un scheduler, un motor de reglas paralelo ni una configuración dinámica de señales: el trabajo ocurre dentro del ciclo `ControlState::scan()` que ya produce la imagen de proceso y evalúa la FSM.

Las reglas de configuración siguen declarando su intención en TOML. En el arranque, el compilador de FSM las valida contra el catálogo y las convierte a una representación tipada. En ejecución no se interpretan strings ni se validan tipos de guards.

## 2. Qué problema resuelve

Hoy los guards sencillos están acoplados a campos de `FsmSceneContext`, mientras que su evidencia se reparte entre presencia, tracking, ocupación, ROI facial y el estado interno de la FSM. Eso tiene tres costes:

- sumar una condición simple exige ampliar el contexto, el evaluador y, normalmente, la telemetría;
- no queda un snapshot único que explique qué evidencia vio la transición;
- la semántica de dato ausente se mezcla con valores booleanos y con convenciones implícitas.

Las señales convierten esa evidencia en un contrato explícito, validado al boot y observable por ciclo. No sustituyen la lógica temporal ya resuelta por el engine, como dwell, prioridad, orden de transiciones, wildcards o la evaluación de zonas.

### Aclaración sobre el dwell de cama

El dwell de tres segundos de `zone_vacated` ya vive en `config/fsm.toml` como `min_duration_ms = 3_000`. Sigue siendo un guard de zona especializado y sigue siendo configurable allí. Las señales no intentan convertir zonas en tags genéricos en esta migración.

## 3. Estado actual y límite de la migración

La fuente actual de los guards simples es `FsmSceneContext` en `core/mana-control/src/fsm/engine.rs`. `ControlState::update_context()`, en `core/mana-control/src/scan.rs`, ya calcula presencia, cara seleccionada, confianza, dwell, borde, corrida del modelo y cardinalidad. El engine mantiene además el latch `face_was_inside`.

El catálogo inicial tendrá **nueve tags**, no once:

- ocho señales base provienen del contexto actual más el conteo crudo de personas;
- una señal derivada representa el latch `face_was_inside`;
- las **once** son los guards simples actuales que se migran, no las señales.

Esta distinción elimina la inconsistencia que había en los documentos previos: ocho tags base no alcanzan para migrar `FaceWasInside` y `FaceWasNotInside` sin perder la semántica de historial.

## 4. Diseño de módulos y propiedad

La propiedad queda dentro de `mana-control`, donde ya viven tanto la evidencia como el evaluador de FSM.

~~~text
core/mana-control/src/
  domain.rs                  SignalTag
  signals/
    mod.rs                   API pública interna del módulo
    catalog.rs               catálogo estático y descriptores
    value.rs                 SignalKind, SignalValue, Ratio, SignalOp
    table.rs                 SignalTable y SceneSignalsSnapshot
  fsm/
    guard.rs                 FsmGuard::Signal
    program.rs               ProgramGuard::Signal y compilación
    engine.rs                evaluación de Signal y latch derivado
  scan.rs                    producción de señales y SceneEvent
~~~

Responsabilidades:

| Componente | Responsabilidad |
|---|---|
| `SignalTag` | Identificador de dominio de una señal; no registra tags por sí solo. |
| `SignalCatalog` | Fuente de verdad de nombres, tipos, presencia, labels permitidos y versión. |
| `SignalTable` | Snapshot lógico del ciclo, validado y ordenado de forma determinista. |
| `FsmGuard` | Forma deserializada desde TOML, aún apta para acumular errores de configuración. |
| `ProgramGuard` | Forma compilada, tipada y lista para evaluación en T2. |
| `ControlState` | Productor de las ocho señales base en el mismo punto que hoy actualiza el contexto. |
| `FsmEngine` | Conserva y publica la señal derivada de historial antes de evaluar guards normales. |
| T3/logger | Serializa el snapshot ya construido; nunca modifica ni interpreta la tabla. |

No se añadirá un crate, una dependencia de registro, un archivo TOML para tags ni un mecanismo de hot reload. El catálogo v1 es parte del binario y evoluciona con el código productor.

## 5. Modelo de dominio

### 5.1 Identidad y catálogo

Se añadirá `SignalTag` a `core/mana-control/src/domain.rs` mediante el macro `domain_id!`, siguiendo `StateId` y `ZoneId`.

`domain_id!` define el nuevo tipo de dominio sobre `DomStr`; no define un registro global ni constantes de tags. En particular, `SignalTag` debe ser `Clone`, `Eq`, `Hash` y `Ord`, pero no `Copy`.

El catálogo se expone mediante una función estática, por ejemplo:

~~~rust
pub fn scene_signal_catalog() -> &'static SignalCatalog
~~~

Su implementación usa una colección ordenada y una versión explícita:

~~~rust
pub struct SignalCatalog {
    version: u32,
    descriptors: BTreeMap<SignalTag, SignalDescriptor>,
}

pub struct SignalDescriptor {
    kind: SignalKind,
    presence: SignalPresence,
    allowed_labels: BTreeSet<String>,
}
~~~

`SignalPresence` declara cuándo una señal puede estar presente: siempre, con una
cara seleccionada, con ROI de dwell configurado o mientras la FSM está activa.
La metadata describe el contrato; no produce valores ni sustituye la ausencia
real del snapshot.

La versión inicial es `1`. Un consumidor que no conozca un tag nuevo no debe inferir comportamiento: conserva lo que entiende y considera desconocido el resto. Quitar, renombrar o cambiar tipo/rango de un tag exige un tag nuevo y coexistencia durante la migración.

### 5.2 Tipos y operadores

La implementación refleja el contrato de la spec:

~~~rust
pub enum SignalKind {
    Bool,
    Count,
    Ratio,
    Label,
}

pub enum SignalValue {
    Bool(bool),
    Count(u64),
    Ratio(Ratio),
    Label(String),
}

pub enum SignalOp {
    Eq,
    Ne,
    Gte,
    Lte,
    Gt,
    Lt,
}
~~~

`Ratio` encapsula el `f32`: su constructor rechaza valores no finitos y valores fuera de `[0.0, 1.0]`. No expone un constructor libre, no deriva `PartialEq` y no permite una comparación exacta desde un guard. Esto impide que una igualdad de punto flotante llegue por accidente al runtime.

La matriz de operadores se mantiene tal como define la spec:

| Tipo | Operadores válidos |
|---|---|
| `Bool` | `==`, `!=` |
| `Count` | `==`, `!=`, `>=`, `<=`, `>`, `<` |
| `Ratio` | `>=`, `<=`, `>`, `<` |
| `Label` | `==`, `!=` |

Todas las comparaciones se realizan solamente entre valores ya tipados. El evaluador no convierte texto a número, etiquetas a booleanos ni ratios a enteros.

### 5.3 Tabla y snapshot

La tabla usa `BTreeMap<SignalTag, SignalValue>`, no `HashMap`.

Con nueve claves el coste es irrelevante, y la ordenación estable da snapshots reproducibles, logs deterministas y tests menos frágiles. Una tabla es nueva en cada `scan()`; nunca se arrastra una señal del ciclo anterior.

La API mínima es:

~~~rust
pub struct SignalTable {
    values: BTreeMap<SignalTag, SignalValue>,
}

impl SignalTable {
    pub fn insert(
        &mut self,
        catalog: &SignalCatalog,
        tag: SignalTag,
        value: SignalValue,
    ) -> Result<(), SignalTableError>;

    pub fn matches(
        &self,
        tag: &SignalTag,
        op: SignalOp,
        expected: &SignalValue,
    ) -> Result<bool, CompareError>;

    pub fn snapshot(
        &self,
        catalog: &SignalCatalog,
    ) -> SceneSignalsSnapshot;
}
~~~

`insert()` verifica que el tag exista y que el tipo y, para `Label`, el dominio permitido coincidan con el descriptor. No normaliza, no recorta valores y no convierte una violación en `false`. `matches()` devuelve `Ok(false)` para una señal ausente y propaga como `Err` las combinaciones inválidas de tipo u operador.

Una falla de inserción revela un defecto interno entre productor y catálogo. La política decidida es:

1. no se evalúan guards genéricos contra una tabla incompleta;
2. el control lleva la FSM al estado seguro mediante el mecanismo existente de recuperación;
3. se emite un diagnóstico `SignalFault` con el tag, el productor y la causa;
4. no se reutiliza el último valor válido.

Es distinta de una señal ausente: ausencia es un estado normal de la evidencia; `SignalFault` es un fallo de programa u operación.

## 6. Catálogo v1

El catálogo inicial y sus productores quedan fijados así:

| Tag | Tipo | Presencia | Productor / semántica |
|---|---|---|---|
| `persona.presente` | Bool | siempre | `raw_person_count > 0`. |
| `persona.cantidad` | Count | siempre | Conteo crudo de personas detectadas; no es cantidad de tracks confirmados. |
| `cara.presente` | Bool | siempre | Existe una cara seleccionada para la muestra actual. |
| `cara.confianza` | Ratio | solo con cara seleccionada | Confianza de la cara seleccionada. Ausente si no hay cara. |
| `cara.en_dwell` | Bool | solo con ROI de dwell configurado | `true` si la cara intersecta la ROI; `false` si hay ROI pero no hay cara o está fuera. |
| `cara.en_borde` | Bool | siempre | La persona seleccionada está en el borde; `false` si no hay persona o no aplica la ROI. |
| `cara.modelo_corrio` | Bool | siempre | El modelo facial corrió para la muestra actual. |
| `ocupacion.cardinalidad` | Label | siempre | Exactamente `empty`, `single` o `multiple`. |
| `cara.estuvo_dentro` | Bool | mientras FSM esté activa | Valor actual del latch `face_was_inside` del engine. |

La distinción para `cara.en_dwell` es deliberada:

- `false`: existe ROI de dwell y la condición no se cumple;
- ausente: ese establecimiento no configuró ROI de dwell.

Por lo tanto, un guard `cara.en_dwell == false` no debe convertirse en una forma implícita de “no existe ROI”.

La señal `cara.estuvo_dentro` se publica después de aplicar las reglas existentes del latch: depende del estado de FSM y de presencia facial, se limpia al detectar cardinalidad múltiple y mantiene sus reglas actuales de reset y transición. Es una señal derivada de estado del engine, no de un detector.

`persona.cantidad` y `cara.modelo_corrio` no reemplazan un guard actual; entran desde v1 para observabilidad y reglas futuras. Los demás tags permiten migrar los guards simples existentes.

## 7. Guard Signal y compilación

### 7.1 Forma de configuración

La forma TOML elegida es pequeña y legible:

~~~toml
[[transitions]]
from = "detecting"
to = "face_detected"
guards = [
  { type = "signal", tag = "cara.confianza", op = ">=", value = 0.80 },
  { type = "signal", tag = "ocupacion.cardinalidad", op = "==", value = "single" },
]
~~~

El tipo del tag determina el tipo de `value`:

- booleano TOML para `Bool`;
- entero no negativo para `Count`;
- número finito dentro de `[0, 1]` para `Ratio`;
- string para `Label`.

No se añade un campo `value.type` redundante. Para ratio se aceptan literales numéricos TOML enteros o decimales, siempre que cumplan el rango; el programa compilado conserva un `Ratio`, no un float arbitrario.

### 7.2 Representación antes y después de compilar

La configuración debe poder representar errores de usuario sin fallar en el primer guard. Por eso las dos capas siguen separadas:

~~~rust
pub enum FsmGuard {
    // Guards existentes...
    Signal {
        tag: String,
        op: String,
        value: SignalLiteral,
    },
}

pub enum SignalLiteral {
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(String),
}

pub enum ProgramGuard {
    // Guards existentes...
    Signal {
        tag: SignalTag,
        op: SignalOp,
        value: SignalValue,
    },
}
~~~

`FsmProgram::compile_with_references()` obtiene el catálogo estático internamente. No se agrega un parámetro adicional al bootstrap ni se permite que cada deployment inyecte otro catálogo.

La compilación acumula errores con el contexto de la transición, el índice del guard, el tag y el tipo/valor esperado. Rechaza:

- tag desconocido;
- operador desconocido o incompatible;
- tipo de literal incompatible;
- ratio fuera de rango, no finito o con igualdad;
- count negativo;
- label fuera del conjunto declarado;
- sintaxis `Signal` en transición wildcard.

Un catálogo o FSM inválido no produce un `FsmProgram` ejecutable. No hay validación diferida, tags libres ni configuración de guards en caliente.

### 7.3 Restricción de wildcards

`Signal` queda prohibido cuando `from = "*"`.

Los wildcards existentes mantienen su propósito de seguridad y salud, y su secuencia actual de evaluación no se modifica. Esta restricción también garantiza que el snapshot de señales representa exactamente la evaluación normal de estado para el ciclo, sin crear ambigüedad entre las dos pasadas actuales del engine.

## 8. Ciclo de control

La ubicación de la lógica es el `scan()` actual:

~~~text
ProcessImage congelada + instante inyectado
  -> presencia / tracker / ocupación / zonas
  -> update_context() y producción de ocho señales base
  -> actualización del latch de FSM y cara.estuvo_dentro
  -> snapshot congelado
  -> guards Signal + guards especializados
  -> lote de SceneEvent
  -> T3 / logger best effort
~~~

Secuencia concreta:

1. `scan()` trabaja contra la `ProcessImage` ya fechada y el instante inyectado.
2. Los productores existentes actualizan presencia, tracker, ocupación y zonas.
3. `update_context()` conserva su salida durante B-C por compatibilidad y, en el mismo lugar, inserta las ocho señales base.
4. Antes de evaluar la transición normal, `FsmEngine` aplica la lógica existente de `face_was_inside` y coloca `cara.estuvo_dentro` en esa tabla.
5. La tabla se congela y todos los `ProgramGuard::Signal` de esa evaluación leen el mismo snapshot.
6. Los guards de zona, salud y profundidad siguen leyendo sus motores y snapshots específicos.
7. La FSM conserva su prioridad, su orden de catálogo y su dwell actuales. `Signal` cambia la fuente de ciertos predicados, no el motor temporal ni la política de transiciones.
8. En D, el snapshot se adjunta al lote de eventos del mismo ciclo.

No se introduce la promesa de “una sola transición por ciclo” en este diseño. El engine conserva exactamente la secuenciación ya establecida; la migración debe demostrar paridad con ella.

## 9. Semántica de ausencia y errores

Una señal ausente no coincide con ningún operador, incluido `!=`.

~~~text
cara.en_dwell == false
  coincide solo si existe una ROI de dwell y la cara está fuera o no está presente.

cara.en_dwell != true
  tampoco coincide si no hay ROI configurada, porque el tag está ausente.
~~~

Esto evita que la ausencia de capacidad o configuración se convierta silenciosamente en evidencia negativa.

| Situación | Resultado |
|---|---|
| Tag ausente | Guard `Signal` no coincide; no es error. |
| Valor presente que no satisface el guard | No hay transición por ese guard; aplica la lógica de dwell existente. |
| Evidencia vieja | Sigue perteneciendo a `Health` y `data_stale`; no se traduce a `Bool(false)`. |
| TOML inválido | Error acumulado en boot; no se ejecuta el programa. |
| Inserción interna inválida | `SignalFault`, estado seguro y diagnóstico; nunca clamping ni reutilización. |
| Falla de logger | Best effort en T3; no bloquea T2. |

## 10. Migración de guards existentes

Las once variantes simples se sustituyen por estas expresiones compiladas:

| Guard actual | Guard Signal equivalente |
|---|---|
| `PersonPresent` | `persona.presente == true` |
| `PersonAbsent` | `persona.presente == false` |
| `FaceDetected { min_confidence }` | `cara.confianza >= min_confidence` |
| `FaceAbsent` | `cara.presente == false` |
| `FaceInDwell` | `cara.en_dwell == true` |
| `FaceNotInDwell` | `cara.en_dwell == false` |
| `FaceAtEdge` | `cara.en_borde == true` |
| `FaceNotAtEdge` | `cara.en_borde == false` |
| `FaceWasInside` | `cara.estuvo_dentro == true` |
| `FaceWasNotInside` | `cara.estuvo_dentro == false` |
| `Cardinality { value }` | `ocupacion.cardinalidad == value` |

La equivalencia de `FaceDetected` se conserva porque `cara.confianza` solo está presente cuando existe cara seleccionada. Una cara ausente no satisface `>=`, por lo que no hace falta componer otro guard.

Los guards especializados que quedan fuera de esta tabla, como zona, salud y profundidad, no se migran en A-D.

## 11. Observabilidad y gemelo digital

En la etapa D se añade un evento de dominio:

~~~rust
SceneEvent::SceneSignals {
    stamp: ControlStamp,
    snapshot: SceneSignalsSnapshot,
}
~~~

Se genera una vez por ciclo, inmediatamente antes de la evaluación normal de la FSM. `ControlStamp` ya aporta `scan_seq`, `evidence_frame_id`, `observations_age_ms` y `depth_age_ms`; no se inventan campos paralelos como `tick` o `timestamp_ms`.

El snapshot incluye:

- `catalog_version: 1`;
- las nueve entradas en orden de tag;
- el tipo de cada entrada;
- el valor cuando está presente;
- `absent: true` cuando el tag declarado no tiene evidencia en ese ciclo.

Ejemplo conceptual:

~~~json
{
  "type": "scene_signals",
  "scan_seq": 481,
  "catalog_version": 1,
  "signals": {
    "cara.confianza": { "kind": "ratio", "value": 0.84 },
    "cara.en_dwell": { "kind": "bool", "value": false },
    "ocupacion.cardinalidad": { "kind": "label", "value": "single" },
    "cara.estuvo_dentro": { "kind": "bool", "value": true }
  }
}
~~~

En la serialización real se incluyen también los tags ausentes, aunque el ejemplo se acorte para lectura. La representación JSON final mantiene orden estable al serializar las entradas; no se serializa un `HashMap<String, _>` arbitrario.

El mapper de `src/logger/event/scene.rs` debe mapear este evento a un `Event::SceneSignals` persistido por defecto con severidad informativa. El objetivo es auditoría de incidentes: omitirlo como hoy se omiten `Occupancy` y `FsmState` invalidaría la reconstrucción por ciclo.

En D, `FaceDwellLogStrategy` deja de depender de `FsmSceneContext` y consume el snapshot de señales. El fixture o dump plano anterior se retira solo cuando el nuevo evento cubra esa necesidad y los golden tests de prefijo estén actualizados.

## 12. Etapas de entrega

| Etapa | Entregable | Condición de salida |
|---|---|---|
| A | Tipos, catálogo v1, tabla validada y tests unitarios. | No cambia la evaluación de FSM. |
| B | Doble producción de contexto y ocho señales base; inserción de la señal de latch; pruebas de paridad. | El contexto existente sigue gobernando las reglas. |
| C | `FsmGuard::Signal`, compilación tipada y migración de once guards simples. | Configuración inválida falla en boot; zonas/salud/profundidad siguen propias. |
| D | `SceneSignalsSnapshot`, evento y serialización; retiro del contexto plano. | El log permite correlacionar evidencia y transición del mismo ciclo. |

### Pruebas obligatorias

A:

- constructor de `Ratio` para bordes, fuera de rango y no finitos;
- tabla rechaza tag desconocido, tipo incompatible y label inválido;
- orden de snapshot estable;
- la ausencia no coincide con ningún operador.

B:

- paridad de las ocho señales base con los valores actuales de `FsmSceneContext`;
- secuencias de latch para entrada, salida, cardinalidad múltiple y reset;
- no hay contaminación entre ciclos.

C:

- catálogos de blueprint válidos siguen compilando;
- errores acumulados para tag, operador, tipo, label y ratio inválidos;
- cada una de las once migraciones conserva su comportamiento;
- `Signal` wildcard se rechaza.

D:

- JSON de snapshot con valores y ausencias;
- correlación con el mismo `ControlStamp` de la transición;
- golden test de ciclo completo como prefijo: se permite el nuevo evento sin perder la evidencia anterior;
- prueba de que una falla de serialización no bloquea el ciclo de control.

No se agrega QuickCheck ni otro framework de property testing para esta entrega. Los casos de dominio son pequeños, deterministas y quedan cubiertos con pruebas unitarias y de integración existentes.

## 13. Límites explícitos

Esta decisión no incluye:

- convertir zonas, salud o profundidad en señales genéricas;
- hot reload de catálogo o reglas;
- tags definidos libremente por deployment;
- agregación temporal adicional dentro de `SignalTable`;
- cambiar thresholds clínicos o la política de seguridad;
- publicar un protocolo externo de señales;
- cambiar la semántica de wildcards existentes;
- esconder fallos de productor mediante defaults, clamping o valores stale.

## 14. Consecuencias

El costo de la decisión es una capa de dominio más y doble escritura temporal durante B. A cambio, la FSM recibe condiciones configurables y tipadas, las ausencias dejan de ser ambiguas y el sistema puede explicar qué evidencia produjo una transición.

La arquitectura queda deliberadamente conservadora: el catálogo es estático, la tabla es pequeña y local al ciclo, y la FSM sigue siendo la única autoridad de transición. Eso permite migrar sin alterar los invariantes operativos que ya protege el control.
