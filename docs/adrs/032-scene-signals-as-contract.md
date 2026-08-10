# ADR-032: La tabla de señales es un contrato, no una comodidad interna

**Status:** Accepted
**Date:** 2026-08-10
**Reemplaza:** [ADR-031](031-scene-signal-table.md) (Proposed, nunca implementado)

## Qué cambia para el producto

Hoy, **cambiar cuándo suena una alerta clínica es un release**. Si un servicio
quiere que la alerta de salida de cama espere 5 segundos en vez de 3, hay que
editar Rust, recompilar, y desplegar un binario nuevo en el equipo del cuarto.

Después de esto, es editar un TOML.

Tres consecuencias concretas, en orden de peso:

**1. Cada servicio puede tener sus umbrales.** Una cama de UTI y una de sala
general no necesitan el mismo dwell ni la misma distancia de aproximación. Hoy
el binario impone una sola respuesta clínica para todos.

**2. Se puede auditar por qué *no* sonó una alerta.** Hoy el gemelo digital
—qué veía el sistema en cada tick— vive dentro del proceso y solo salen los
eventos que alguien eligió a mano. Peor: el logger **descarta** dos tipos de
evento, así que hay estado que nunca sale. Con señales etiquetadas se puede
volcar la escena completa: en una revisión de incidente, la pregunta "¿el
sistema vio a la persona?" tiene respuesta.

**3. Otros sistemas pueden consumir la escena sin conocer `mana-lite`.** Un
motor de workflows, un tablero, un gateway HL7 hablan de *tags y valores*, no
de variantes de un `enum` de Rust. Hoy integrarse exige leer nuestro código.

## Contexto

[ADR-031](031-scene-signal-table.md) propuso convertir `FsmSceneContext` en una
tabla de señales, y lo justificó por **costo interno de desarrollo**: agregar un
predicado de escena cuesta 6 ediciones en 4 archivos. Puso el disparo en
umbrales de crecimiento — más de 25 variantes de `FsmGuard`, más de 12 campos
de contexto.

Ese encuadre estaba mal, y por eso el ADR quedó dos meses en *Proposed*.

**El costo de las 6 ediciones no es el problema.** Agregar una regla clínica
también exige decidir su semántica, elegir histéresis y dwell, escribir los
tests y validar contra video real. Las 6 ediciones mecánicas son veinte minutos
de una tarea de días. Optimizarlas no mueve la aguja.

Lo que sí la mueve es que **la tabla de señales es una interfaz**, y las
interfaces tienen otra ventana de oportunidad. Un refactor interno mal hecho se
rehace y nadie se entera. Una interfaz construida *después* de que existen los
consumidores obliga a migrarlos. Por eso el criterio de "esperar al umbral" no
aplica: para un contrato, llegar tarde cuesta cualitativamente más que llegar
temprano.

## Decisión

`FsmSceneContext` deja de ser un struct de campos con nombre y pasa a ser una
**tabla de señales etiquetadas** que es **contrato publicado**, no detalle de
implementación.

Se preserva el split `FsmGuard` / `ProgramGuard`. Esa duplicación aparente es
la separación compilar-en-boot / ejecutar-determinista de un PLC: `FsmGuard` es
el texto del programa, `ProgramGuard` es el programa compilado contra los
catálogos. Fusionarlos destruiría la validación de arranque.

Cuatro reglas que se derivan de tratarlo como contrato, y que ADR-031 no tenía:

**Los tags son vocabulario declarado, no strings.** Se usa el mecanismo que ya
existe: `domain_id!` de `mana-id` ([ADR-030](030-shared-mechanism-owned-vocabulary.md)).
Un `SignalTag` declarado por el crate que lo produce. Sin esto, en seis meses
conviven `face_in_dwell` y `faceInDwell` y el contrato no significa nada.

**Los valores llevan semántica, no solo tipo.** `Ratio` no es "un `f32`": es
"de 0 a 1". `Count` no es "un `u32`": es "cuántos". Un consumidor externo tiene
que poder interpretar sin leer nuestro código.

**Hay una regla de evolución, escrita.** Agregar un tag es compatible. Quitarlo
o renombrarlo, no. Cambiarle el tipo, no. Eso se decide ahora, no cuando algo
se rompa en producción.

**El estado completo es observable.** Un evento de volcado del gemelo digital,
emitido con el resto del lote de `scan()`. Cierra de paso el agujero que hoy
obliga a mantener un fixture extra (`multi_actor_cycle.events.txt`) porque el
JSONL no ve `Occupancy` ni `FsmState`.

### Qué se colapsa y qué no

De los 18 guards actuales, **~11 pasan a una sola variante genérica**
`Signal { tag, op, value }`: los 8 de cara, los 2 de persona y `cardinality`.
Todos son "leé un campo del contexto y comparalo".

Los otros **7 conservan variante propia** porque tienen lógica real, no una
comparación: los 4 de zonas necesitan el motor de zonas y sus timers de
histéresis, los 2 de salud leen `Health`, y `depth_rule` lee el snapshot de
profundidad. La tabla no los reemplaza.

Resultado: 18 variantes → 7 propias + 1 genérica.

## Consecuencias

**Positivo.** Una regla clínica nueva pasa de 6 ediciones a 1-2. Se habilitan
reglas por despliegue sin recompilar. El gemelo digital es inspeccionable
entero. La escena tiene un vocabulario que otros sistemas pueden consumir.

**Negativo, y es real.** Se pierde el chequeo exhaustivo del compilador sobre
los predicados genéricos. Hoy `FaceDetected { min_confidence: f32 }` está
tipado: no se puede comparar una confianza contra un booleano, no compila.
Después, el tipo de un tag se conoce en arranque.

Eso **no se acepta como pérdida neta**: se compensa endureciendo
`FsmProgram::compile()`, que ya devuelve `Result<Self, Vec<String>>` y acumula
errores. Un tag inexistente, un tipo incorrecto o un operador que no aplica al
tipo tienen que fallar en boot con un mensaje que diga qué y dónde. Si eso no
está, la decisión no se sostiene.

Es el modelo PLC y es aceptable porque el programa es fijo tras el arranque: el
sistema no acepta reglas nuevas en caliente.

## Referencias

- Spec del contrato: [docs/scene-signals/1-spec.md](../scene-signals/1-spec.md)
- Plan de ejecución: [docs/scene-signals/2-sprints.md](../scene-signals/2-sprints.md)
- ADR-015 (motor FSM), ADR-002 (patrón de catálogo TOML),
  [ADR-027](027-tier-architecture.md) (tiers),
  [ADR-030](030-shared-mechanism-owned-vocabulary.md) (vocabulario)
