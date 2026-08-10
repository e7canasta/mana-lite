# Backlog operativo — Señales de escena

**Estado:** Sprint 3 / Etapa C cerrada; Etapa D pendiente
**Próximo hito:** D — gemelo visible
**Fuente de verdad:** [1-spec.md](1-spec.md), [requirements.md](requirements.md),
[design.md](design.md), [2-sprints.md](2-sprints.md), [3-sprint-1.md](3-sprint-1.md)
y [7-sprint-2-handoff.md](7-sprint-2-handoff.md)

Este archivo es el backlog ejecutable del proyecto. El roadmap describe las
etapas y el diseño explica la arquitectura; aquí se define el orden de trabajo,
los archivos que toca cada tarea y la evidencia necesaria para cerrarla.

## 1. Regla de operación

- Las Etapas B y C están cerradas con código, pruebas y compuerta verificadas. D
  sigue siendo el hito posterior, no trabajo adelantado.
- Una casilla se marca al completar código, pruebas y la verificación indicada.
- Un fallo de compuerta detiene el avance de la etapa; no se regenera un golden
  para hacerlo pasar.
- El worktree puede contener cambios ajenos. Se registran en la línea de base
  y nunca se revierten para cerrar una tarea.
- La implementación sigue los tipos y semántica ya decididos; este backlog no
  reabre catálogo, ausencia, TOML ni observabilidad.

## 2. Mapa de etapas

| Etapa | Estado | Resultado | Documento de cierre |
|---|---|---|---|
| A | **Cerrada** | Catálogo, valores, operadores y tabla sin consumidores productivos. | [6-sprint-1-cierre.md](6-sprint-1-cierre.md) |
| B | **Cerrada** | Doble producción, paridad y latch derivado. | [8-sprint-2-cierre.md](8-sprint-2-cierre.md) |
| C | **Cerrada** | Guard `Signal`, compilación de boot y once migraciones. | [9-sprint-3-cierre.md](9-sprint-3-cierre.md) |
| D | Pendiente | Snapshot observable, logger y retiro del contexto plano. | [2-sprints.md](2-sprints.md#etapa-d--el-gemelo-visible) |

## 3. Sprint 1 — definición de terminado

Sprint 1 termina cuando `mana-control` contiene un vocabulario de nueve
señales, tipos que hacen inválidos los ratios y operadores incompatibles, y una
tabla ordenada que valida inserciones. Nada fuera del módulo de señales la
produce o consume todavía.

**No entra en este sprint:**

- Cambios en `scan()`, `ControlState`, `FsmSceneContext` o
  `FsmEngine`.
- `FsmGuard::Signal`, `ProgramGuard::Signal`, TOML, validación de
  blueprints o migración de guards.
- Eventos `SceneEvent`, logger, JSONL, serialización o fixtures de auditoría.
- Cambios a blueprints, umbrales clínicos, zonas, salud o profundidad.

## 4. Línea de base

### S1-00 — Congelar el punto de partida

**Objetivo:** saber con qué comportamiento y qué cambios ajenos se empieza.

- [x] Registrar `git status --short` antes del primer cambio de código.
- [x] Ejecutar `cargo test -p mana-control` y guardar el resultado de cierre.
- [x] Registrar el estado de `tests/golden/` sin limpiar cambios ajenos.
- [x] Leer [design.md](design.md#5-modelo-de-dominio) y
  [3-sprint-1.md](3-sprint-1.md) antes de definir la API pública.
- [x] Confirmar que no existe un consumidor previo de `SignalTable` fuera del
  módulo que se creará.

**Evidencia de cierre:** línea base de Git y goldens registrada en
[6-sprint-1-cierre.md](6-sprint-1-cierre.md). El resultado de prueba previo no
quedó persistido antes de esta revisión; la suite de cierre y la suite release
quedaron verdes.

## 5. Backlog de implementación

### S1-01 — Abrir la frontera del módulo

**Objetivo:** dar al crate dueño un lugar estable para el vocabulario sin
conectar el lazo de control.

**Archivos previstos**

- `core/mana-control/src/domain.rs`
- `core/mana-control/src/lib.rs`
- `core/mana-control/src/signals/mod.rs`

**Trabajo**

- [x] Declarar `SignalTag` con `domain_id!` en `domain.rs`, siguiendo el
  patrón de `StateId` y `ZoneId`.
- [x] Agregar `pub mod signals;` en `lib.rs`.
- [x] Crear `signals/mod.rs` como frontera de la API de señales.
- [x] Exponer solo los tipos que necesitan las etapas siguientes; dejar los
  detalles de almacenamiento internos.
- [x] No declarar un registry dentro de `domain_id!` ni hacer `SignalTag`
  `Copy`.

**Criterios de aceptación**

- [x] `SignalTag` pertenece a `mana-control` y conserva las características
  de los IDs de dominio existentes.
- [x] El crate compila con el módulo vacío o con sus tipos base.
- [x] `scan()`, FSM y logger no importan el módulo nuevo.

**Dependencia:** S1-00.

### S1-02 — Implementar el catálogo v1

**Objetivo:** hacer explícita la fuente de verdad de tags, tipos, presencia,
labels y versión.

**Archivos previstos**

- `core/mana-control/src/signals/catalog.rs`
- `core/mana-control/src/signals/mod.rs`
- Pruebas colocadas junto al módulo, siguiendo la convención local.

**Trabajo**

- [x] Definir `SignalKind`, `SignalDescriptor` y `SignalCatalog`.
- [x] Implementar `scene_signal_catalog() -> &'static SignalCatalog` con
  versión `1`.
- [x] Declarar los nueve tags canónicos:
  `persona.presente`, `persona.cantidad`, `cara.presente`,
  `cara.confianza`, `cara.en_dwell`, `cara.en_borde`,
  `cara.modelo_corrio`, `ocupacion.cardinalidad` y
  `cara.estuvo_dentro`.
- [x] Declarar `empty`, `single` y `multiple` como único dominio de
  `ocupacion.cardinalidad`.
- [x] Validar pertenencia al catálogo y el formato `dominio.atributo`.
- [x] Mantener una colección ordenada: la futura auditoría exige resultados
  reproducibles.

**Criterios de aceptación**

- [x] Los nueve tags, sus tipos, presencia y labels coinciden con
  [design.md](design.md#6-catalogo-v1).
- [x] Un `SignalTag` formado desde texto no se acepta como tag declarado si
  no existe en el catálogo.
- [x] El catálogo no lee archivos de configuración ni depende del deployment.

**Dependencia:** S1-01.

### S1-03 — Implementar valores y operadores seguros

**Objetivo:** capturar en tipos las restricciones que no deben quedar a cargo de
un guard o de un productor futuro.

**Archivos previstos**

- `core/mana-control/src/signals/value.rs`
- `core/mana-control/src/signals/mod.rs`

**Trabajo**

- [x] Definir `Ratio` como wrapper opaco de `f32`.
- [x] Hacer que el constructor de `Ratio` rechace `NaN`, infinitos y valores
  fuera de `[0.0, 1.0]`.
- [x] Definir `SignalValue::{Bool, Count, Ratio, Label}` con
  `Count(u64)`.
- [x] Definir `SignalOp::{Eq, Ne, Gte, Lte, Gt, Lt}`.
- [x] Implementar la matriz `SignalKind × SignalOp` y una comparación tipada
  que no convierta valores implícitamente.
- [x] Evitar `PartialEq` para `Ratio` y cualquier API que permita igualdad
  exacta entre ratios.
- [x] Definir errores de construcción y compatibilidad que puedan ser usados
  luego por tabla y compilador de FSM.

**Criterios de aceptación**

- [x] `Ratio(0.0)` y `Ratio(1.0)` son válidos.
- [x] Ratios negativos, mayores a uno, `NaN` e infinitos retornan error.
- [x] `Eq` y `Ne` son incompatibles con `Ratio`.
- [x] No se puede comparar un `Bool` con `Gte` ni un `Label` con `Lt`.
- [x] La comparación de valores no expone coerciones de texto, entero o float.

**Dependencia:** S1-02.

### S1-04 — Implementar tabla y snapshot deterministas

**Objetivo:** construir un contenedor local al ciclo que preserve ausencia y
rechace evidencia que contradiga el catálogo.

**Archivos previstos**

- `core/mana-control/src/signals/table.rs`
- `core/mana-control/src/signals/mod.rs`

**Trabajo**

- [x] Implementar `SignalTable` sobre `BTreeMap<SignalTag, SignalValue>`.
- [x] Implementar `insert(catalog, tag, value)` que valide tag, tipo y
  labels antes de aceptar la entrada.
- [x] Exponer lectura por tag y recorrido estable.
- [x] Definir `SceneSignalsSnapshot` como vista inmutable y ordenada, sin
  JSON ni `SceneEvent` en esta etapa.
- [x] Hacer que una tabla nueva empiece vacía; no agregar `clear()` como
  mecanismo de reutilización entre ticks.
- [x] Representar ausencia por falta de entrada, nunca como un valor por
  defecto.
- [x] Dejar definidos los errores de inserción para que B pueda convertir una
  incoherencia de productor en `SignalFault`.

**Criterios de aceptación**

- [x] La misma secuencia de inserciones se itera de forma determinista.
- [x] Un tag desconocido, tipo incorrecto o label inválido se rechaza.
- [x] Un tag ausente se puede distinguir de `Bool(false)`.
- [x] El snapshot no requiere que T2, T3, FSM o logger estén conectados.

**Dependencia:** S1-02 y S1-03.

### S1-05 — Cubrir el contrato con pruebas

**Objetivo:** convertir las decisiones de A en pruebas que fallen ante una
regresión de semántica.

**Archivos previstos**

- Pruebas unitarias junto a `signals`.
- Sin nuevas dependencias de testing salvo necesidad demostrada.

**Trabajo**

- [x] Probar el catálogo completo: versión, nueve tags, tipo, presencia y labels.
- [x] Probar nombre válido e inválido, tag no declarado y label no emitible.
- [x] Probar los límites y no finitos de `Ratio`.
- [x] Probar cada combinación válida e inválida de tipo y operador.
- [x] Probar que la igualdad de ratio se rechaza por compatibilidad de
  operadores y que `Ratio` no deriva igualdad exacta.
- [x] Probar `insert`, lectura, ausencia, tipo incorrecto, label inválido y
  orden estable de tabla/snapshot.
- [x] Probar que no hay valor heredado entre dos tablas nuevas.
- [x] Mantener las pruebas de A libres de `scan()`, configuración de FSM,
  TOML o logger.

**Criterios de aceptación**

- [x] La suite cubre los bordes de contrato sin tests frágiles de formato.
- [x] Un cambio que vuelva `cara.en_dwell` igual a `false` por ausencia
  tiene una prueba que falla.
- [x] No se introduce `trybuild` u otro framework solo para demostrar una
  restricción que la API ya expresa y los tests de compatibilidad cubren.

**Dependencia:** S1-04.

### S1-06 — Revisión de superficie y documentación

**Objetivo:** asegurar que A no filtró consumo productivo ni cambió el contrato
documentado.

**Trabajo**

- [x] Revisar la API pública de `signals`: mínima, documentada y coherente
  con el diseño.
- [x] Verificar que no hay `SignalTable` fuera de `signals` y sus pruebas.
- [x] Comparar implementación con [requirements.md](requirements.md), en
  particular los requisitos RF-01, RF-02 y RF-03.
- [x] Actualizar la documentación solo si el código revela una contradicción
  real; no reabrir decisiones con refactors cosméticos.
- [x] Registrar cualquier diferencia entre diseño y código como bloqueo antes
  de marcar A completa.

**Dependencia:** S1-05.

### S1-07 — Compuerta de cierre de Sprint 1

**Objetivo:** demostrar que el contrato está listo para B sin cambiar el
comportamiento clínico.

- [x] Ejecutar `cargo test -p mana-control`.
- [x] Ejecutar `cargo clippy --workspace -- -D warnings`.
- [x] Ejecutar `git diff --check`.
- [x] Comparar `tests/golden/` contra la línea de base de S1-00; no debe haber
  cambios atribuibles a Sprint 1.
- [x] Verificar que `rg -n "SignalTable" core/mana-control/src` solo encuentre
  el módulo `signals` y sus pruebas.
- [x] Revisar el diff para confirmar que no toca `scan.rs`, `fsm/`,
  blueprints, logger o fixtures.
- [x] Publicar evidencia de comandos y cerrar el sprint con el resultado de
  cada criterio de [3-sprint-1.md](3-sprint-1.md#compuerta-de-cierre).

**Excepción de línea base:** la orden estricta de Clippy fue ejecutada, pero el
workspace ya contiene warnings en `mana-geometry`, módulos legacy de
`mana-control` y el binario principal. No hay warnings reportados en
`signals/`; esta deuda queda fuera de Sprint 1 y se registra en
[6-sprint-1-cierre.md](6-sprint-1-cierre.md).

**Dependencia:** S1-06.

## 6. Orden y entregables

| Orden | Tarea | Entregable verificable |
|---|---|---|
| 0 | S1-00 | Línea de base registrada. |
| 1 | S1-01 | `SignalTag` y frontera `signals`. |
| 2 | S1-02 | Catálogo v1 con nueve descriptores y presencia. |
| 3 | S1-03 | Valores, `Ratio` y operadores seguros. |
| 4 | S1-04 | Tabla y snapshot ordenados y validados. |
| 5 | S1-05 | Pruebas de contrato. |
| 6 | S1-06 | Revisión de alcance y documentación. |
| 7 | S1-07 | Compuerta funcional y evidencia de cierre. |

S1-02 y S1-03 pueden avanzar en paralelo después de S1-01 si se acuerda la
superficie compartida. S1-04 debe esperar ambos; no se paraleliza con
producción, guards o logger porque esos trabajos pertenecen a etapas futuras.

## 7. Sprint 2 — entrada y primer objetivo

Sprint 2 queda abierto después del cierre funcional de S1-07. Su primer objetivo
es producir las ocho señales base desde `update_context()` y luego sumar
`cara.estuvo_dentro` desde el latch de `FsmEngine`. La tabla sigue sin ser
autoridad de guards hasta que la paridad esté demostrada.

- [x] S2-01: añadir producción paralela de las ocho señales base.
- [x] S2-02: publicar `cara.estuvo_dentro` después de actualizar el latch.
- [x] S2-03: agregar paridad tick a tick para `multi_actor_cycle`.
- [x] S2-04: comprobar que cada ciclo crea una tabla nueva y conserva ausencia.
- [x] S2-05: ejecutar la compuerta B sin cambiar los goldens.

La guía operativa de entrada está en
[7-sprint-2-handoff.md](7-sprint-2-handoff.md). La evidencia de cierre está en
[8-sprint-2-cierre.md](8-sprint-2-cierre.md). La evidencia de C está en
[9-sprint-3-cierre.md](9-sprint-3-cierre.md). El detalle contractual de B-C-D
permanece en [2-sprints.md](2-sprints.md).

## 8. Sprint 3 — guard genérico

La Etapa C cambia la fuente de evidencia de los predicados simples, no la
política de la FSM. Los siete guards especializados de zonas, Health y
profundidad permanecen con sus motores propios.

- [x] S3-01: agregar `FsmGuard::Signal`, `SignalLiteral` y `ProgramGuard::Signal`.
- [x] S3-02: validar tags, operadores, tipos, rangos, labels y wildcards en boot.
- [x] S3-03: evaluar todos los guards genéricos contra un snapshot congelado.
- [x] S3-04: migrar los once guards simples, un guard por commit.
- [x] S3-05: verificar debug, release, `ffmpeg` y goldens sin cambios.

La evidencia de cierre está en [9-sprint-3-cierre.md](9-sprint-3-cierre.md).
