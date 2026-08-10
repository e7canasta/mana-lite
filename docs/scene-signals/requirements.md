# Requisitos de producto — Señales de escena

**Estado:** requisitos consolidados para implementación
**Producto:** mana-lite / control de escena
**Contrato normativo:** [1-spec.md](1-spec.md)
**Diseño de referencia:** [design.md](design.md)
**Plan de entrega:** [2-sprints.md](2-sprints.md)

## 1. Objetivo

Permitir que un blueprint exprese una condición clínica nueva sobre evidencia de
escena ya producida sin agregar una variante de guard en Rust, y dejar un
registro suficiente para reconstruir qué evidencia vio la FSM en cada ciclo.

El cambio no altera la decisión clínica vigente. En particular, el dwell de la
zona de cama ya se configura en el blueprint y sigue siendo responsabilidad del
motor de zonas. El alcance de señales es exponer evidencia simple, tipada y
auditable para que pueda usarse en condiciones de FSM.

## 2. Resultados de negocio

Al completar A-D, el producto debe ofrecer estos resultados:

| Audiencia | Resultado |
|---|---|
| Ingeniería clínica | Puede componer condiciones sobre tags existentes desde `fsm.toml`, con validación antes del arranque. |
| Operación y soporte | Puede correlacionar un ciclo de la FSM con las señales presentes y ausentes que explican una decisión. |
| Desarrollo | Puede sumar evidencia declarada sin crear una variante de guard para cada comparación simple. |
| Integraciones futuras | Puede consumir un vocabulario versionado sin conocer `FsmSceneContext` ni otros structs internos. |

## 3. Alcance

### Incluido

- Catálogo estático, versionado y declarado de señales.
- Tabla nueva por ciclo de control, con ausencia distinguible de un valor falso.
- Guard genérico `Signal` para reglas de FSM configuradas en TOML.
- Validación acumulada de reglas antes de comenzar el control.
- Migración de los once guards simples existentes.
- Snapshot completo y persistido de señales para auditoría por ciclo.
- Retiro de `FsmSceneContext` plano después de comprobar la migración.

### Excluido

- Convertir guards de zonas, salud o profundidad en señales genéricas.
- Recargar catálogo o reglas sin reiniciar y validar el programa.
- Crear tags libres por deployment o desde TOML.
- Alterar thresholds clínicos, dwell, prioridad, orden o política de seguridad.
- Definir un protocolo externo, retención o compresión de logs.
- Convertir señales en un cache entre ciclos o en un segundo motor de reglas.

## 4. Términos y actores

| Término | Definición |
|---|---|
| Señal | Evidencia de escena expresada como tag y valor tipado durante un ciclo. |
| Catálogo | Fuente de verdad de tags, tipos, labels válidos y versión. |
| Tabla | Valores de señales materializados para un único ciclo de `scan()`. |
| Snapshot | Vista congelada de tabla usada por la FSM y, en D, emitida al log. |
| Ausencia | Un tag declarado sin evidencia aplicable en ese ciclo; no equivale a `false`. |
| Guard `Signal` | Predicado `(tag, operador, valor)` ya compilado y tipado. |
| `FsmGuard` | Regla deserializada del blueprint, que todavía puede tener errores de configuración. |
| `ProgramGuard` | Regla validada y ejecutable por el engine. |
| T2 | Lazo de control que produce y evalúa el snapshot. |
| T3 | Reporte y logging best-effort que no participa en la decisión. |

## 5. Requisitos funcionales

### RF-01 — Vocabulario publicado y estable

El sistema debe exponer un catálogo v1 perteneciente a `mana-control`. Los
tags no son strings libres: envolver texto en `SignalTag` no lo convierte en
una señal conocida.

**Criterios de aceptación**

1. El catálogo v1 contiene exactamente los nueve tags de la tabla siguiente.
2. Cada tag tiene un tipo, una semántica de presencia y, cuando corresponde,
   labels cerrados.
3. Agregar un tag o un label compatible es una evolución aditiva; quitar,
   renombrar o cambiar tipo/rango requiere un tag nuevo y convivencia.
4. El catálogo expone versión `1` junto con el snapshot observable.
5. El catálogo no se define en un TOML de deployment ni admite hot reload.

| Tag | Tipo | Presencia | Semántica |
|---|---|---|---|
| `persona.presente` | Bool | siempre | `raw_person_count > 0`. |
| `persona.cantidad` | Count | siempre | Conteo crudo de personas, no cantidad de tracks confirmados. |
| `cara.presente` | Bool | siempre | Hay una cara seleccionada en la muestra actual. |
| `cara.confianza` | Ratio | solo con cara | Confianza de la cara seleccionada. |
| `cara.en_dwell` | Bool | solo con ROI de dwell | Dentro/fuera de la ROI configurada. |
| `cara.en_borde` | Bool | siempre | La persona seleccionada está en el borde; `false` si no aplica. |
| `cara.modelo_corrio` | Bool | siempre | El modelo facial corrió para la muestra actual. |
| `ocupacion.cardinalidad` | Label | siempre | Uno de `empty`, `single` o `multiple`. |
| `cara.estuvo_dentro` | Bool | mientras FSM esté activa | Latch de historial del engine. |

### RF-02 — Tipos, comparación y ausencia

El sistema debe impedir que configuraciones ambiguas o numéricamente incorrectas
lleguen al lazo de control.

**Criterios de aceptación**

1. Solo existen los tipos `Bool`, `Count`, `Ratio` y `Label`.
2. `Count` representa un entero no negativo.
3. `Ratio` solo acepta valores finitos de `0.0` a `1.0`, inclusive, y no
   puede compararse por igualdad exacta.
4. La matriz de operadores es la siguiente:

| Tipo | Operadores permitidos |
|---|---|
| `Bool` | `==`, `!=` |
| `Count` | `==`, `!=`, `>=`, `<=`, `>`, `<` |
| `Ratio` | `>=`, `<=`, `>`, `<` |
| `Label` | `==`, `!=` |

5. Una señal ausente no coincide con ningún guard, incluido uno con `!=`.
6. `cara.en_dwell == false` coincide solo si existe la ROI de dwell y la
   condición fue negativa; un deployment sin ROI mantiene el tag ausente.
7. `cara.confianza` es ausente cuando no se seleccionó una cara. No se
   materializa con `0.0` para representar ausencia.
8. Datos viejos o una cámara sin señal siguen siendo responsabilidad de
   `Health`; no se convierten en señales booleanas negativas.

### RF-03 — Producción de snapshot por ciclo

La tabla debe representar evidencia del mismo ciclo que evalúa la FSM y no
arrastrar valores implícitos de ciclos anteriores.

**Criterios de aceptación**

1. Cada `scan()` crea una tabla nueva a partir de la `ProcessImage` fechada
   y del instante inyectado.
2. Durante B, `update_context()` produce en paralelo el contexto legado y las
   ocho señales base, preservando las mismas semánticas.
3. Antes de evaluar guards normales, `FsmEngine` actualiza su latch existente
   e incorpora `cara.estuvo_dentro` al mismo snapshot.
4. Todos los `ProgramGuard::Signal` de esa evaluación leen una única vista
   congelada de la tabla.
5. La tabla no reutiliza un valor previo, no completa ausencias con defaults y
   no acepta coerciones de tipo.
6. Las entradas y el snapshot tienen orden determinista para que un mismo ciclo
   produzca la misma evidencia observable.

### RF-04 — Configuración y compilación de guards

Un blueprint debe poder declarar condiciones genéricas sobre tags conocidos sin
trasladar validación al runtime.

La forma soportada en TOML es:

~~~toml
guards = [
  { type = "signal", tag = "cara.confianza", op = ">=", value = 0.80 },
  { type = "signal", tag = "ocupacion.cardinalidad", op = "==", value = "single" },
]
~~~

**Criterios de aceptación**

1. `FsmGuard::Signal` conserva el texto de `tag`, `op` y `value` para
   permitir diagnósticos acumulados.
2. `ProgramGuard::Signal` contiene `SignalTag`, `SignalOp` y
   `SignalValue` ya tipados.
3. El tipo del tag determina el tipo permitido de `value`; TOML no usa un
   wrapper redundante como `{ type = "ratio", value = 0.7 }`.
4. La compilación de boot rechaza y acumula: tag desconocido, operador
   incompatible, tipo de literal incompatible, count negativo, ratio no finito
   o fuera de rango, igualdad de ratio y label no emitible.
5. Cada error identifica transición, índice de guard, tag y expectativa.
6. Si existe cualquier error, no se inicia un programa de FSM parcialmente
   compilado.
7. `Signal` no está permitido en una transición wildcard; los wildcards
   existentes conservan su semántica de seguridad.
8. No hay interpretación de strings, validación de tipos ni recarga de reglas
   durante el tick.

### RF-05 — Compatibilidad de comportamiento

La migración cambia la fuente de evidencia de predicados simples, no la
política del engine.

**Criterios de aceptación**

1. Se migran exactamente los once guards siguientes:

| Guard vigente | Expresión `Signal` |
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

2. `FaceDetected` se representa solo con `cara.confianza >= umbral`:
   la ausencia de confianza ya impide que la comparación coincida.
3. Los guards `zone_present`, `zone_occupied`, `zone_vacated`,
   `all_zones_vacant`, `data_stale`, `data_fresh` y `depth_rule`
   conservan variante y motor propios.
4. Dwell, orden de catálogo, prioridad y secuenciación actual de la FSM se
   mantienen sin cambio.
5. La migración no promete ni introduce una cantidad nueva de transiciones por
   ciclo.

### RF-06 — Auditoría del gemelo digital

El sistema debe poder explicar qué evidencia se evaluó en el ciclo asociado a
una transición o a la ausencia de transición.

**Criterios de aceptación**

1. En D, `scan()` agrega una vez por ciclo
   `SceneEvent::SceneSignals { stamp, snapshot }` al lote de eventos.
2. `stamp` es el `ControlStamp` existente y permite correlacionar
   `scan_seq`, frame de evidencia y edades con la evaluación de FSM.
3. El snapshot incluye `catalog_version = 1` y los nueve tags declarados,
   tanto presentes como ausentes.
4. Cada entrada incluye su tipo y, según aplique, un valor o `absent: true`.
5. La salida tiene orden estable y se serializa en T3 como
   `Event::SceneSignals` con nivel informativo y persistencia por defecto.
6. El reporte es best-effort: una falla de serialización o de sink no bloquea
   T2 ni modifica la decisión del ciclo.
7. `FaceDwellLogStrategy` deja de depender del contexto plano antes de
   retirar `FsmSceneContext`.
8. El JSONL no usa un `timestamp_ms` ni un contador paralelo inventado: usa
   el sello de control y el timestamp que ya provee el pipeline de logging.

### RF-07 — Seguridad operacional

La ausencia esperable de evidencia y una incoherencia interna del productor son
situaciones distintas y deben producir respuestas distintas.

**Criterios de aceptación**

1. Una señal opcional ausente produce no coincidencia, no un error ni una ruta
   segura por sí misma.
2. Un productor que intenta insertar un tag desconocido, tipo incompatible,
   ratio inválido o label fuera de catálogo genera `SignalFault`.
3. Ante `SignalFault` no se evalúan guards genéricos con una tabla parcial,
   no se hace clamping y no se reutiliza el último valor válido.
4. El control sigue la ruta de estado seguro existente y emite un diagnóstico
   con tag, productor y causa.
5. El manejo de evidencia stale permanece en `Health` y sus guards
   especializados.

### RF-08 — Entrega y evidencia de aceptación

La entrega se divide en A-D para aislar riesgo y demostrar paridad antes de que
la tabla gobierne decisiones.

| Etapa | Resultado requerido | Evidencia mínima |
|---|---|---|
| A | Catálogo, valores, operadores y tabla sin productores ni consumidores. | Unit tests de tipo, rango, operador, orden y ausencia. |
| B | Doble producción y señal de latch. | Paridad contra contexto y pruebas de secuencia del latch. |
| C | Guard genérico y once migraciones. | Errores de boot, paridad por guard y goldens sin cambio. |
| D | Snapshot, evento, serialización y retiro del contexto plano. | Log correlacionable y golden previo como prefijo. |

**Criterios de aceptación**

1. En A-C, los tres goldens existentes son byte-idénticos.
2. En D, el golden anterior es prefijo del nuevo y las únicas líneas agregadas
   corresponden a observabilidad.
3. La validación cubre los cinco rechazos de configuración: tag, operador,
   tipo/valor, ratio y label.
4. La etapa B prueba ausencia y `false` como estados distintos.
5. La etapa C prueba por separado cada migración de guard y el rechazo de
   `Signal` en wildcard.
6. La etapa D prueba valores, ausencias, orden estable, correlación por
   `ControlStamp` y la degradación best-effort del logger.
7. El cierre de cada etapa ejecuta los comandos y compuertas de
   [2-sprints.md](2-sprints.md).

## 6. Requisitos de calidad

1. El camino T2 no realiza I/O de red o disco por construir o evaluar señales.
2. La tabla v1 queda acotada a nueve tags y se materializa una vez por ciclo.
3. El reporte y cualquier serialización quedan fuera de la decisión del engine.
4. La evidencia de una misma entrada y programa debe ser determinista, incluido
   el orden del snapshot.
5. No se adopta una métrica inventada de latencia, bytes o volumen de logs como
   criterio de aceptación. La etapa D debe medir el costo real del evento antes
   de fijar una política de retención o un SLO operativo.

## 7. No objetivos y decisiones descartadas del borrador

Para evitar que este documento vuelva a abrir decisiones cerradas:

- El catálogo v1 no tiene ocho tags: tiene nueve, porque el latch
  `cara.estuvo_dentro` conserva la semántica de historial de la FSM.
- La tabla no usa `HashMap` como contrato de observabilidad; su orden debe ser
  determinista. La estructura concreta se fija en [design.md](design.md).
- `domain_id!` crea el tipo `SignalTag`, pero no funciona como registry de
  tags declarados. El catálogo cumple ese rol.
- `FaceDetected` no necesita un segundo guard de presencia: la confianza es
  ausente cuando no hay cara.
- El evento no se llama `SceneSignalsDump` y no inventa `tick` ni
  `timestamp_ms`; usa `SceneEvent::SceneSignals` y `ControlStamp`.
- TOML no declara el tipo dentro de `value`; el descriptor del tag lo
  determina durante la compilación.
- Compresión, retención, acceso a logs, presupuestos de memoria y volumen diario
  no se declaran sin una medición del sistema real. Son decisiones operativas
  posteriores a D, no requisitos de esta migración.

## 8. Trazabilidad

| Necesidad | Documento que la detalla |
|---|---|
| Contrato público, tipos y evolución | [1-spec.md](1-spec.md) |
| Contexto, límites y audiencias | [4-big-picture.md](4-big-picture.md) |
| Ciclo operativo de T2/T3 | [5-engine-funcional.md](5-engine-funcional.md) |
| Diseño Rust, catálogo y serialización | [design.md](design.md) |
| Etapas, compuertas y sprint inicial | [2-sprints.md](2-sprints.md) y [3-sprint-1.md](3-sprint-1.md) |

Este documento define **qué debe ser cierto para aceptar el producto**. No
duplica la implementación de structs, módulos o funciones; esas decisiones
viven en el diseño técnico.
