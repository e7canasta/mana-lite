# Handoff de Sprint 3: Solicitudes Urgentes Cooperativas

**Fecha de cierre:** 2026-08-12
**Estado:** cerrado en `c902d69`
**Ultimo sprint cerrado:** Sprint 3, solicitudes urgentes cooperativas
**Proposito:** permitir que un modelo pida una ejecucion fuera de su intervalo sin
crear workers, bloquear el control ni perder una solicitud transitoria.

Este documento es el punto de entrada de la proxima sesion. No hace falta
reconstruir el contexto leyendo todo el repositorio: leer este handoff, la spec y
los archivos de la lista de abajo alcanza para empezar.

## 1. Foto Actual

La arquitectura tiene tres duenos de ejecucion:

```text
ingesta RTSP/decode  ->  Slot<RawKeyframe>  ->  hilo de percepcion
                                                        |
                                                        v
                                               cascade secuencial
                                                        |
                                      Slot<PerceptionOutput> + eventos
                                                        |
                                                        v
                                               control fijo @ 5 Hz
                                                        |
                                      Slot<ControlDirective> hacia percepcion
```

El control nunca debe esperar a percepcion. Percepcion procesa el keyframe mas
fresco, ejecuta roots y children secuencialmente y conserva `latest-wins`.

`ControlDirective` vive en el tier T3, en `src/app/perception.rs`, y hoy tiene
solamente:

```text
models: Vec<String>
tracks: Vec<Track>
occupancy: Option<...>
fsm_state: Option<String>
```

La directiva viaja por `Slot<ControlDirective>` porque es una muestra: una
directiva vieja se puede reemplazar. Una request transitoria **no puede** viajar
por ese mismo slot.

`CascadeScheduler` vive en T1, en
`core/mana-perception/src/cascade.rs`. Mantiene reglas, orden, regiones,
intervalos, `last_started_at` y la cola durable de requests urgentes.

La implementación de Sprint 3 ya cubre el contrato, persistencia, cola
transitoria, prioridad, TTL, congelamiento por keyframe, consumo one-shot y
starvation idempotente. No hay todavía un productor semántico face/pose: esa
decisión pertenece a Sprint 4.

## 2. Estado Verificado

- `cargo test --workspace --release`: verde en la compuerta final de este sprint.
- Paquete raíz: 155 tests verdes en la última corrida de la librería.
- `mana-perception`: 34 tests verdes en la última corrida del crate.
- `mana-control`: 160 tests verdes.
- `mana-geometry`: 72 tests verdes.
- `mana-media`: 17 tests verdes.
- `mana-perception`: 34 tests verdes.
- `git diff --check`: limpio.
- `cargo fmt --check`: conserva solamente diferencias preexistentes en archivos
  fuera de este sprint.
- Los perfiles ONNX FP16 `192` y `320` ya estan presentes en el catalogo local;
  el workshop de Sprint 2 esta preparado pero su corrida fisica sigue pendiente.

El worktree ya estaba sucio con cambios previos del baseline, blueprints,
workshop y documentacion. No hacer `reset`, `checkout` ni revertir cambios que no
pertenezcan al Sprint 3.

## 3. Objetivo Y Puerta De Salida

La puerta de Sprint 3 es:

> face puede pedir pose fuera de su periodo y la request llega a un punto seguro
> de ejecucion, se consume una sola vez o expira una sola vez, y nunca bloquea
> el lazo de control.

La primera implementacion debe probar la mecanica con requests sinteticas o
derivadas de una directiva. La semantica de incertidumbre face/pose y la
validacion cruzada pertenecen a Sprint 4.

## 4. Contrato Propuesto

El minimo normativo de `InferenceRequest`, ya fijado en `spec.md`, es:

```text
model_key
reason
priority
requested_at
expires_at
```

Forma sugerida en el runtime:

```rust
pub struct InferenceRequest {
    pub model_key: String,
    pub reason: String,
    pub priority: u8,
    pub requested_at: Instant,
    pub expires_at: Instant,
}
```

Reglas del contrato:

- `priority` mas alto gana; en empate gana la request mas antigua.
- `expires_at` debe ser posterior a `requested_at`.
- La request debe tener un TTL acotado por una constante del scheduler.
- Una request de modelo desconocido, deshabilitado o incompatible se rechaza al
  ingresar y no llega al loop de inferencia.
- Una request valida salta solamente `interval_min_ms`.
- Una request no salta `requires`, clase, cantidad, confianza, region, tracking,
  crop ni orden topologico.
- Una request no inicia un modelo en el mismo keyframe en el que fue producida
  por otra inferencia. Se congela el conjunto de urgencias al inicio del ciclo;
  las nuevas quedan para el siguiente keyframe.
- Se consume al marcar el inicio, antes de llamar al backend.
- Un fallo del backend despues del inicio consume igualmente la request.
- Una request expirada se elimina y se cuenta una sola vez.
- No se debe reutilizar una request consumida ni generar catch-up.

## 5. Dos Clases De Request

### 5.1 Persistente

Una request persistente es parte del estado deseado del control. Se deriva de
`ControlDirective` y puede viajar por el slot porque se reconstruye en cada
directiva:

```text
ControlDirective.urgent_requests
              |
              v
refresh_directive()
              |
              v
scheduler.set_persistent_requests(...)
```

La directiva siguiente puede reemplazar el conjunto anterior. Una lista vacia
debe limpiar las requests persistentes; no debe dejar una urgencia zombie.

### 5.2 Transitoria

Una request transitoria no puede viajar por `Slot<ControlDirective>` porque una
request puede perderse cuando llega otra directiva. Debe tener una cola durable:

```text
producer -> VecDeque<InferenceRequest> -> scheduler
```

Para el primer corte, la cola puede ser propiedad del `CascadeScheduler` y
recibir requests producidas dentro del hilo de percepcion. Si una fuente futura
del control necesita emitir requests transitorias, agregar un canal `mpsc` en
bootstrap, no otro slot:

```text
control Sender<InferenceRequest>
             |
             v
percepcion Receiver<InferenceRequest> -> VecDeque del scheduler
```

El consumo del receiver debe ser no bloqueante en cada keyframe.

## 6. Algoritmo De Scheduling

Mantener el orden topologico actual. El algoritmo esperado es:

1. Al refrescar una directiva, reemplazar el conjunto persistente y drenar las
   requests transitorias disponibles sin bloquear.
2. Al comienzo de `run_inference`, eliminar requests expiradas y registrar cada
   expiracion una sola vez.
3. Congelar una vista de requests validas para este keyframe.
4. Formar el conjunto de modelos normales mas los modelos pedidos urgentemente,
   filtrando catalogo, habilitacion y `disabled_tasks`.
5. Ordenar roots antes que children con `CascadeScheduler::ordered`.
6. Permitir como maximo una admision urgente por keyframe en la primera version.
   Esto evita que una fuente de urgencias mate toda la politica normal.
7. Para el modelo candidato urgente, aplicar todos los gates de cascade. Si no
   hay target, la request permanece pendiente hasta su expiracion.
8. Si el modelo es elegible, consumir la request, registrar espera y ejecutar.
9. Llamar `mark_started` antes de `InferEngine::run`; el inicio urgente reinicia
   el intervalo normal del modelo.
10. Las requests generadas durante este ciclo quedan disponibles desde el
    siguiente keyframe.

La urgencia debe bypass-ear el intervalo, no el gate. No llamar a un modelo
desde otro modelo: el productor publica una request y el scheduler decide.

## 7. Anti-Starvation Minimo

Implementar estas restricciones en Sprint 3, sin anticipar Sprint 5:

- maximo una ejecucion urgente por keyframe;
- maximo una request valida pendiente por modelo y por razon equivalente;
- prioridad descendente y antiguedad ascendente para resolver competencia;
- TTL maximo configurado, por ejemplo 5 segundos, validado al ingresar;
- una request urgente no puede impedir que se evaluen los modelos normales del
  conjunto elegido;
- la ejecucion urgente consume el intervalo normal del modelo;
- no reintentar una request consumida aunque el backend falle.

Si estas reglas no alcanzan para varias urgencias concurrentes, documentarlo y
dejar la politica global de degradacion para Sprint 5. No introducir workers ni
preempcion como solucion local.

## 8. Metricas A Completar

Sprint 2 dejo estos campos y metodos; Sprint 3 ya conecto el productor de
requests sinteticas/directivas:

```text
urgent
urgent_expired
tick_infer_urgent()
tick_infer_urgent_expired()
```

En Sprint 3 no cambiar el significado de `urgent`: debe seguir significando
ejecucion urgente atendida. Agregar campos separados para solicitudes y espera:

```text
urgent_requests
urgent_wait_samples
urgent_wait_min_ms
urgent_wait_p50_ms
urgent_wait_p95_ms
urgent_wait_max_ms
urgent_expired
urgent_starvation
```

La espera se mide desde `requested_at` hasta el inicio. La expiracion se mide
desde la request, no desde el ultimo keyframe. Si se agrega `urgent_starvation`,
definir primero una condicion verificable, por ejemplo una request valida que
cruza al menos dos keyframes sin poder iniciar y aun no expiro.

Publicar los campos globales y por modelo en:

- `src/metrics/mod.rs`;
- `src/pipeline.rs`;
- `src/logger/serialize/basic.rs`;
- `src/config/observability.rs` y `config/metrics.toml`.

No reciclar `not_due`, `gated` ni `skip` para representar espera urgente.

## 9. Archivos Y Puntos De Entrada

### Scheduler T1

- `core/mana-perception/src/cascade.rs`
  - `CascadeScheduler` mantiene el estado temporal.
  - `is_due()` decide cadencia normal.
  - `mark_started()` devuelve `CascadeStartTiming` y fija el inicio.
  - agregar aqui la cola, validacion, prioridad, expiracion y consumo one-shot.

### Directiva Y Thread De Percepcion

- `src/app/perception.rs`
  - `ControlDirective` es el contrato T3 hacia percepcion.
  - `refresh_directive()` recibe el conjunto persistente.
  - `run()` procesa un keyframe por vuelta.
  - `PerceptionPorts` define los bordes entre hilos.
- `src/app/inference.rs`
  - `run_inference()` congela requests y recorre roots/children.
  - `run_root_models()` y `run_child_models()` aplican la cadencia.
  - `run_scheduled_model()` marca el inicio antes del backend.
- `src/app/mod.rs`
  - `publish_directive()` arma la directiva en cada scan.
  - `ControlPorts` es el lado control del slot.
- `src/app/bootstrap/mod.rs`
  - crea slots, canales y `PerceptionPorts`.
  - si se agrega un canal transitorio, conectarlo aqui sin bloquear el scan.

### Reporte

- `src/metrics/mod.rs`: acumuladores y reportes.
- `src/pipeline.rs`: lineas textuales y flags.
- `src/logger/serialize/basic.rs`: JSONL.
- `src/config/observability.rs`: toggles.
- `config/metrics.toml`: defaults operativos.

### Documentacion

- `docs/subprojects/cooperative-inference-scheduler/spec.md`: contrato
  normativo.
- `docs/subprojects/cooperative-inference-scheduler/technical-memory.md`:
  decisiones y limites.
- `docs/subprojects/cooperative-inference-scheduler/roadmap.md`: puerta de salida.
- `workshop/scenarios/11-inference-capacity/`: escenario de capacidad y ventana
  larga; no es requisito para probar la cola con tests sinteticos.

## 10. Plan De Implementacion

### Paso 1: contrato y cola — completado

- Crear `InferenceRequest` y las validaciones de tiempo/prioridad.
- Agregar estado persistente y `VecDeque` transitoria al scheduler.
- Anadir tests de deduplicacion, prioridad, TTL y expiracion unica.

### Paso 2: directiva y consumo — completado

- Agregar `urgent_requests` a `ControlDirective`.
- Conservar el reemplazo latest-wins para el conjunto persistente.
- Integrar la cola transitoria sin bloquear el hilo de control.
- Hacer que requests validas agreguen su modelo al conjunto solicitado sin
  saltarse enabled/catalogo.

### Paso 3: ciclo de inferencia — completado

- Congelar requests al inicio del keyframe.
- Mantener gates de parent/track/class/ROI.
- Permitir bypass de intervalo una sola vez.
- Consumir al inicio, antes del backend.
- Verificar que una request generada durante la inferencia no corre en el mismo
  keyframe.

### Paso 4: metricas — completado

- Agregar contador de solicitudes y distribucion de espera.
- Registrar expiraciones y starvation de forma idempotente.
- Exponer text y JSONL.
- Mantener el significado actual de `urgent` y `urgent_expired`.

### Paso 5: acceptance — completado

- Tests unitarios de scheduler y metricas.
- Test de `ControlDirective` persistente.
- Test de cola transitoria que sobreviva a dos keyframes.
- Test de request que salta intervalo pero no gate.
- Test de expiracion una sola vez.
- Test de prioridad y limite de una urgente por keyframe.
- Golden sin requests identico al actual.
- `cargo test --workspace --release`.

## 11. Criterios De Aceptacion

- Una request valida para `pose-standard` inicia aunque su `interval_min_ms`
  todavia no haya vencido.
- La misma request no inicia dos veces.
- Una request no inicia si no hay target de cascade, aunque sea urgente.
- Una request expirada no inicia y cuenta una sola expiracion.
- Una request transitoria no se pierde por una directiva nueva.
- Una request producida durante un keyframe no se ejecuta en ese mismo keyframe.
- Una inferencia urgente no interrumpe otra en curso.
- Una urgencia no cambia la cadencia del control ni introduce un lock sostenido.
- Sin requests, los modelos, el orden, los goldens y la compatibilidad permanecen
  iguales.
- Las metricas permiten distinguir request atendida, espera, expiracion y
  starvation.
- `cargo test --workspace --release` permanece verde.

## 12. Lo Que No Hacer

- No crear un worker por modelo.
- No agregar dependencia de `mana-perception` a `mana-control`.
- No poner requests transitorias en `Slot<ControlDirective>`.
- No permitir que urgencia saltee gates de parent o tracking.
- No ejecutar same-frame dinamico en este sprint.
- No mover masks, keypoints ni payloads crudos al FSM.
- No activar intervalos nuevos en blueprints de produccion como parte del
  contrato de urgencias.
- No usar el workshop como sustituto de los tests deterministas.
- No revertir cambios previos del worktree.

## 13. Primer Comando De La Proxima Sesion

Desde la raíz del repositorio:

```sh
git status --short
cargo test --workspace --release
```

Luego leer solamente:

```text
docs/subprojects/cooperative-inference-scheduler/sprints/sprint-03-handoff.md
docs/subprojects/cooperative-inference-scheduler/spec.md secciones 7 y 9
src/app/perception.rs: ControlDirective, refresh_directive, PerceptionPorts
src/app/inference.rs: run_inference, run_root_models, run_child_models
core/mana-perception/src/cascade.rs: CascadeScheduler, mark_started
```

El runtime ya está implementado y el commit `c902d69` está creado. La corrida
física de pesos `192/320` queda pendiente explícita. La siguiente sesión debe usar
`sprints/sprint-04-handoff.md`; no comenzar con same-frame dinámico.
