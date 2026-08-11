# Registro ADR — Programa De Señales De Escena

**ID del registro:** MANA-SIG-ADR
**Alcance:** decisiones tomadas durante las etapas A-D
**Estado general:** implementadas, salvo las deudas indicadas en
[deuda.md](deuda.md)

Este archivo es un índice operativo de decisiones. Las ADR históricas de mayor
alcance siguen siendo [ADR-031](../adrs/031-scene-signal-table.md) y
[ADR-032](../adrs/032-scene-signals-as-contract.md).

## ADR-SIG-001 — Catalogo estatico v1

**Etapa:** A
**Estado:** aceptada e implementada

**Contexto:** los guards simples necesitaban un vocabulario estable, pero los
tags libres harían imposible validar un deployment antes de arrancar.

**Decision:** el productor declara un catálogo estático v1 de nueve tags con
tipo, presencia y labels permitidos. El catálogo vive en `mana-control` y no se
carga dinámicamente desde TOML.

**Consecuencias:** el programa se puede validar en boot y el wire lleva
`catalog_version = 1`; no hay hot reload ni extensión libre.

## ADR-SIG-002 — Tabla nueva y snapshot congelado por ciclo

**Etapa:** A-B
**Estado:** aceptada e implementada

**Contexto:** reutilizar la tabla anterior puede filtrar valores stale a un tick
posterior y confundir ausencia con `false` o cero.

**Decision:** cada scan crea una `SignalTable` nueva y produce un
`SceneSignalsSnapshot` inmutable. La iteración usa `BTreeMap` y contiene todos
los tags declarados, presentes o ausentes.

**Consecuencias:** el orden observable es determinista y la ausencia es
explícita; el costo es una pequeña reconstrucción por tick.

## ADR-SIG-003 — Ausencia no es valor negativo

**Etapa:** A-B
**Estado:** aceptada e implementada

**Contexto:** no tener ROI facial o no tener cara visible son situaciones
distintas de observar `false`.

**Decision:** un tag ausente no coincide con ningún operador, incluido `!=`.
El serializador usa `absent: true`; nunca inventa `false`, cero, label o valor
del tick anterior.

**Consecuencias:** los consumidores deben manejar ausencia explícita y los
guards no pueden usar `!=` como prueba de disponibilidad.

## ADR-SIG-004 — Ratio tipado y sin igualdad clínica

**Etapa:** A-C
**Estado:** aceptada e implementada

**Contexto:** confianza y proporciones necesitan rango y comparación ordenada;
la igualdad exacta de float no es una regla clínica segura.

**Decision:** `Ratio` sólo acepta valores finitos en `[0, 1]`. Se permiten
comparaciones ordenadas y se rechaza `==`/`!=`. La serialización usa una API
pública de lectura, sin constructor público de ratios inválidos.

**Consecuencias:** un valor inválido es defecto de productor/configuración y
las reglas deben usar umbral, no igualdad.

## ADR-SIG-005 — Guard generico compilado en boot

**Etapa:** C
**Estado:** aceptada e implementada

**Contexto:** los once guards simples repetían campos de contexto y cada nueva
condición requería una variante Rust.

**Decision:** `FsmGuard::Signal` se deserializa como literal no tipado y
`FsmProgram::compile_with_references()` resuelve tag, operador, tipo, rango y
label antes de crear el programa ejecutable. Los errores se acumulan.

**Consecuencias:** el runtime evalúa una regla tipada y fija; zonas, Health y
profundidad conservan sus motores especializados. Los guards `Signal` no se
permiten en transiciones wildcard.

## ADR-SIG-006 — Latch temporal propiedad de la FSM

**Etapa:** B-C-D
**Estado:** aceptada e implementada

**Contexto:** `cara.estuvo_dentro` depende del historial de la FSM, no sólo de
la observación del tick.

**Decision:** `FsmEngine` mantiene `face_was_inside`, actualiza el latch usando
el snapshot y publica el valor como la novena señal. Cardinalidad multiple y
reset limpian el latch según las reglas existentes.

**Consecuencias:** el logger no inventa temporalidad y el snapshot observado por
los guards contiene el mismo valor de latch del ciclo. Las APIs de contexto
legacy quedan como adaptadores temporales.

## ADR-SIG-007 — Un evento de dominio antes de la evaluación FSM

**Etapa:** D
**Estado:** aceptada e implementada

**Contexto:** registrar sólo transiciones no permite explicar por qué una regla
no coincidió.

**Decision:** `scan()` emite exactamente un
`SceneEvent::SceneSignals { stamp, snapshot }` después de construir el snapshot
y antes de evaluar la FSM. Se usa el `ControlStamp` del mismo input.

**Consecuencias:** incluso sin `FsmEngine` se publica el snapshot completo con
`cara.estuvo_dentro` ausente. No se agregan relojes, ticks ni contadores
paralelos.

## ADR-SIG-008 — Frontera T2/T3 y logger best-effort

**Etapa:** D
**Estado:** aceptada e implementada

**Contexto:** serializar dentro de `mana-control` convertiría I/O en parte de
la decisión clínica.

**Decision:** T2 produce el evento de dominio sin JSON ni sink. App mapea a
`Event::SceneSignals`; el logger filtra y serializa en T3. Una falla de sink
puede perder reporte, pero no bloquear ni modificar T2.

**Consecuencias:** el evento es auditable y desacoplado del backend. La prueba
de sink real queda registrada como DEBT-05.

## ADR-SIG-009 — Wire format ordenado y persistencia informativa

**Etapa:** D
**Estado:** aceptada e implementada

**Contexto:** un objeto JSON basado en `HashMap` no garantiza orden textual y
ocultar el evento detrás de debug rompería el caso forense.

**Decision:** `Event::SceneSignals` serializa un array `signals` ordenado por
tag. Cada entrada lleva `kind` y `value` o `absent: true`. El evento tiene nivel
mínimo informativo y `scene_signals_events = true` por defecto.

**Consecuencias:** el JSONL es determinista y aparece con `jsonl_level = info`.
Los perfiles pueden filtrar explícitamente el evento, pero una configuración
operativa debe mantenerlo activo.

## ADR-SIG-010 — Mantener `FaceDwell` durante la migracion

**Etapa:** D
**Estado:** aceptada e implementada

**Contexto:** dashboards y diagnósticos existentes consumen `face_dwell`,
mientras el gemelo visible aporta una explicación más completa.

**Decision:** ambos eventos conviven durante D. `FaceDwellLogStrategy` toma sus
campos de escena desde `SceneSignalsSnapshot`; `FsmSnapshot` conserva estado,
dwell y timers.

**Consecuencias:** no se rompe el consumidor legacy y se evita duplicar una
fuente de verdad. La retirada requiere cerrar DEBT-06 y una ventana de
compatibilidad.

## ADR-SIG-011 — Contexto plano fuera de la ruta productiva

**Etapa:** D
**Estado:** aceptada e implementada parcialmente

**Contexto:** el contexto plano en `ControlState` era una segunda fuente para
App y logger, pero algunos tests y adaptadores del `FsmEngine` aún lo usan.

**Decision:** eliminarlo de `ControlState`, App y logger. La ruta productiva usa
`evaluate_snapshot_at()` y `evaluate_wildcard_snapshot_at()`; las funciones de
contexto del engine permanecen como compatibilidad interna temporal.

**Consecuencias:** la evidencia clínica productiva tiene una sola fuente. La
retirada completa de las APIs legacy queda en DEBT-02.

## ADR-SIG-012 — Goldens aditivos y secuencia legacy estable

**Etapa:** D
**Estado:** aceptada e implementada

**Contexto:** agregar un evento antes de una transición puede hacer que una
regeneración textual oculte cambios en eventos existentes.

**Decision:** el batch bruto conserva `SceneSignals` antes de la evaluación FSM.
El mapper T3 difiere el registro `scene_signals` hasta después de los eventos
legacy del scan para mantener su secuencia contigua. Los goldens agregan sólo
eventos de observabilidad.

**Consecuencias:** se puede comparar la secuencia legacy sin reinterpretar el
orden del dominio. El comparador general de streams queda como DEBT-09.

## 3. Reglas Para Reabrir Una ADR

Una ADR sólo se reabre si cambia al menos uno de estos contratos:

- semántica clínica o estado seguro;
- nombre, tipo o presencia de un tag;
- formato JSONL consumido externamente;
- frontera T2/T3 o política de bloqueo por sink;
- compatibilidad de blueprints y catálogos.

Un refactor interno, una mejora de rendimiento o una corrección de documentación
debe actualizar la implementación y la deuda correspondiente sin crear una
decisión clínica implícita.
