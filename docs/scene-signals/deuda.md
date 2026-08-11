# Deuda Tecnica — Señales De Escena

**ID:** MANA-SIG-DEBT
**Alcance:** etapas A, B, C y D
**Estado:** registro vivo; no bloquea la operación de D
**Regla:** una deuda no autoriza cambiar la semántica clínica ni regenerar un
golden sin evidencia

Este documento separa lo que quedó deliberadamente fuera de A-D de un defecto
operativo. Cada item tiene un disparador y una condición de cierre; no se debe
resolver por limpieza cosmética durante una emergencia clínica.

## 1. Resumen

| ID | Prioridad | Área | Estado | Disparador de trabajo |
|---|---|---|---|---|
| DEBT-01 | P1 | Clippy | Abierta, línea base | se habilita una ventana de saneamiento del workspace |
| DEBT-02 | P1 | Contexto legacy | Abierta, acotada | se define la retirada de APIs de contexto del `FsmEngine` |
| DEBT-03 | P1 | SignalFault | Abierta | un productor interno pueda fallar sin panic y entrar en safe state |
| DEBT-04 | P1 | Wire schema | Abierta | aparece un consumidor externo o una segunda versión del catálogo |
| DEBT-05 | P2 | Sink failure | Parcial | se requiere prueba de disco/stdout lleno en CI |
| DEBT-06 | P2 | Evento FaceDwell | Abierta | `scene_signals` cubra formalmente todos los consumidores del evento legacy |
| DEBT-07 | P2 | Evolución de catálogo | Abierta | se propone agregar o retirar un tag |
| DEBT-08 | P2 | Validación operativa | Abierta | se necesita validar un blueprint sin iniciar RTSP |
| DEBT-09 | P2 | Goldens | Abierta | cambie el orden o el esquema de observabilidad |
| DEBT-10 | P2 | Runbook de plataforma | Abierta | se defina despliegue multi-cámara y retención de logs |

## 2. Items

### DEBT-01 — Línea base de Clippy

**Origen:** compuertas de A-C y D.
**Situación:** `cargo clippy --workspace -- -D warnings` continúa afectado por
warnings preexistentes en `mana-geometry`, `mana-control` y componentes legacy.
La Etapa D no agregó filtros globales para ocultarlos.

**Riesgo:** un warning nuevo puede quedar mezclado con la línea base y reducir
la señal de revisión.

**Cierre:** registrar baseline por crate, eliminar warnings por grupos pequeños,
ejecutar `-D warnings` por crate y finalmente volver a habilitar la compuerta del
workspace. No mezclar cambios clínicos.

### DEBT-02 — Adaptadores de `FsmSceneContext`

**Origen:** Etapa D.
**Situación:** `ControlState`, App y `FaceDwellLogStrategy` ya no mantienen el
contexto plano en la ruta productiva. `FsmEngine` conserva métodos de evaluación
con contexto para las pruebas y consumidores internos existentes.

**Riesgo:** una nueva integración puede escoger accidentalmente la API legacy y
crear una segunda fuente de evidencia.

**Cierre:** migrar los consumidores restantes a `evaluate_snapshot_at()` y
`evaluate_wildcard_snapshot_at()`, marcar las APIs de contexto como internas o
deprecated y retirarlas cuando no queden usos productivos.

### DEBT-03 — Ruta explícita de `SignalFault`

**Origen:** Etapas A-B.
**Situación:** la inserción de señales ya valida catálogo, tipo y labels, pero
la ruta productiva convierte un defecto de productor/catalogo en `panic!` al
usar `insert_signal(...).unwrap_or_else(...)`. El diseño funcional menciona
`SignalFault` y estado seguro, pero esa salida todavía no es un evento de
dominio estructurado.

**Riesgo:** una señal interna inválida puede reiniciar el proceso en vez de
producir diagnóstico y safe state controlado.

**Cierre:** devolver un error de scan tipado, emitir diagnóstico Health sin
coerción ni reutilización de valores y demostrar que la FSM entra en su estado
seguro. Mantener la política de fail-closed.

### DEBT-04 — Contrato formal del wire JSONL

**Origen:** Etapa D.
**Situación:** el serializador es manual y `scene_signals` usa un array ordenado
para conservar el orden textual. El contrato está probado con fixtures y
assertions, pero no existe todavía un schema externo versionado para validadores
de terceros.

**Riesgo:** un consumidor puede depender de nombres, nullabilidad u orden sin que
esa dependencia esté declarada.

**Cierre:** publicar un schema JSONL para `scene_signals`, documentar campos
obligatorios/opcionales y agregar validación de fixture en CI. El orden textual
debe seguir siendo determinista aunque el consumidor lo trate como colección.

### DEBT-05 — Inyección de fallas reales de sink

**Origen:** D-03.
**Situación:** existe una prueba de fanout degradado y el handler real registra
warnings cuando un write falla. No hay todavía una prueba determinista de
disco lleno, stdout cerrado o rotación fallida.

**Riesgo:** una ruta de error de I/O puede comportarse distinto de la ruta de
handler que sólo descarta el evento.

**Cierre:** inyectar un writer fallido en el logger, demostrar que T2 completa
el ciclo y verificar que otro handler o el proceso continúan según la política
de best-effort.

### DEBT-06 — Retirada del `FaceDwell` legacy

**Origen:** D-04.
**Situación:** `FaceDwell` y `SceneSignals` conviven. El primero conserva estado,
timers y un resumen útil para dashboards existentes; sus campos de escena ya se
leen desde el snapshot.

**Riesgo:** duplicación de volumen y dos formatos para una misma investigación.

**Cierre:** inventariar consumidores de `face_dwell`, demostrar que todos
pueden reconstruir la información desde `scene_signals` + `fsm`, versionar la
eliminación y mantener una ventana de compatibilidad.

### DEBT-07 — Evolución del catálogo v1

**Origen:** Etapa A y contrato de D.
**Situación:** el catálogo estático tiene nueve tags, `catalog_version = 1` y
no admite hot reload ni tags libres.

**Riesgo:** agregar un tag, cambiar un tipo o quitar un tag puede romper
consumidores silenciosamente si sólo se modifica Rust.

**Cierre:** crear una versión de catálogo nueva para cambios incompatibles,
publicar migración y mantener convivencia cuando existan consumidores externos.
Agregar tags es compatible sólo si la ausencia/desconocido se trata como
no-match.

### DEBT-08 — Validador de blueprint sin iniciar el proceso

**Origen:** C-D.
**Situación:** la validación ocurre durante bootstrap y la suite de tests carga
catálogos, pero la operación no tiene todavía un comando de validación offline
para un deployment concreto.

**Riesgo:** una configuración inválida puede descubrirse al arrancar contra una
fuente RTSP real.

**Cierre:** agregar un subcomando o herramienta que valide paths, modelos,
blueprint, zonas, depth, FSM y métricas sin abrir cámara ni sinks.

### DEBT-09 — Golden y diff de observabilidad

**Origen:** D-06.
**Situación:** los goldens actuales verifican el JSONL y el batch bruto, y las
aserciones prueban nueve tags, stamps y ausencias. Aún no existe una herramienta
general que compare dos streams ignorando únicamente eventos aditivos.

**Riesgo:** una regeneración manual puede ocultar una modificación clínica o un
cambio de orden legacy.

**Cierre:** crear un comparador que quite sólo `scene_signals`, compare la
secuencia legacy y reporte cambios de decisión por separado.

### DEBT-10 — Operación multi-cámara y retención

**Origen:** límites explícitos de A-D.
**Situación:** la implementación documenta una instancia, rotación local y
`ControlStamp`; no define todavía identidad de stream en el wire, retención,
backpressure, centralización ni alertas de plataforma.

**Riesgo:** mezclar logs de cámaras o retener evidencia indefinidamente.

**Cierre:** definir stream identity, storage policy, exportación, retención,
alertas y procedimiento de borrado conforme a la política del despliegue.

## 3. No Es Deuda De Este Sprint

Estos puntos son límites intencionales, no tareas fallidas:

- hot reload de reglas o catálogo;
- tags libres definidos por deployment;
- convertir zonas, Health o profundidad en señales genéricas;
- compresión, protocolo externo o política de retención;
- cambiar thresholds, dwell, prioridades o estados clínicos;
- reemplazar el logger best-effort por un camino que bloquee T2.

## 4. Regla De Priorizacion

Resolver primero DEBT-01 a DEBT-04 si se prepara una integración externa o un
release clínico. Resolver DEBT-05 a DEBT-09 para endurecimiento de CI y
observabilidad. Resolver DEBT-10 antes de operar más de una cámara o centralizar
evidencia.
