# Modelo funcional — Motor de señales de escena

**Estado:** diseño operativo decidido; la spec prevalece ante cualquier
diferencia. Describe el estado objetivo al completar las etapas A a D.

*Big picture: [4-big-picture.md](4-big-picture.md) · Contrato: [1-spec.md](1-spec.md) · Plan: [2-sprints.md](2-sprints.md) · Diseño técnico: [design.md](design.md) · Decisión: [ADR-032](../adrs/032-scene-signals-as-contract.md)*

## Propósito

Este documento traduce el contrato de señales a un ciclo de vida operativo:
arranque, compilación, tick, evaluación y observabilidad. No fija nombres de
estructuras de Rust, sintaxis de TOML ni un transporte externo.

El motor de señales no es un scheduler nuevo ni un proceso paralelo. Es una
responsabilidad dentro del paso de control de T2 que ya corre en cada scan.

## Posición en el sistema

~~~text
Blueprint y catálogos declarados
             │
             │ validar y compilar en arranque
             ▼
        programa de FSM inmutable

Imagen de proceso fechada
             │
             │ tick del programa de control
             ▼
productores de señales ──► tabla del tick ──► guards Signal ──► FSM
             │                                           │
zonas, Health y profundidad ──► guards propios ──────────┘
                                                         │
                                                         ▼
                                             lote de eventos de escena
                                                         │
                                                         ▼
                                          reporte T3 best-effort, en D
~~~

T1 produce observaciones, no escribe directamente en la tabla. T3 reporta la
salida del lazo y no participa en la decisión ni bloquea el tick.

## Participantes y propiedad

| Participante | Propiedad funcional |
|---|---|
| Catálogo de señales | el crate productor declara tags, tipos, labels y versión |
| FsmGuard | texto de la regla que proviene del blueprint |
| ProgramGuard | regla ya compilada y válida para ejecutar durante el tick |
| Tabla de señales | snapshot lógico de la escena para un tick |
| Imagen de proceso | evidencia congelada y fechada que alimenta el programa |
| Guards especializados | zonas, Health y profundidad con sus propios motores |
| Evento de escena | salida que T3 puede serializar sin afectar T2 |

Se conserva la separación entre FsmGuard y ProgramGuard. La primera representa
la intención de configuración; la segunda representa un programa que ya pasó
las validaciones de arranque.

## Arranque y compilación

El arranque carga el blueprint, el catálogo de señales y los catálogos de
referencia necesarios. Luego compila el programa del FSM antes de ejecutar el
primer tick.

Para cada guard genérico, la compilación comprueba:

1. Que el tag exista en el catálogo del productor.
2. Que el operador aplique al tipo del tag.
3. Que un Ratio no se compare por igualdad.
4. Que el valor respete el rango semántico del tipo.
5. Que el valor de un Label pueda ser emitido por ese productor.

Los errores se acumulan. Cada uno debe indicar transición, tag y expectativa,
para que una configuración inválida se corrija en una sola intervención. Si hay
errores, no existe un programa parcialmente compilado ni se inicia el engine.

Después de arrancar, el programa queda fijo. No hay recarga de reglas ni
validación dinámica durante la ejecución.

## Un tick de control

Cada tick usa la imagen de proceso vigente y un instante inyectado. Produce una
tabla nueva; no reutiliza la tabla del tick anterior como cache implícito.

1. El lazo actualiza tracker, presencia, ocupación y la evidencia auxiliar.
2. Los productores materializan las señales declaradas con los valores de ese
   tick.
3. Zonas, Health y profundidad exponen sus resultados a los guards que los
   necesitan.
4. Los guards genéricos leen la tabla; los especializados conservan sus motores
   y snapshots propios.
5. El FSM aplica el orden, prioridades y dwell que ya posee.
6. El tick devuelve estado, transición cuando corresponda y el lote de eventos.

La variante Signal reemplaza una clase de predicado; no crea timers, prioridades
ni un segundo motor de transiciones.

## Snapshot de señales

Una tabla contiene pares tag → valor. Ejemplo conceptual, no sintaxis de
configuración:

~~~text
persona.presente       = Bool(true)
persona.cantidad       = Count(1)
cara.confianza         = Ratio(0.87)
cara.en_dwell          = ausente
ocupacion.cardinalidad = Label("single")
~~~

La tabla se itera de manera determinista para que la misma entrada y el mismo
programa produzcan el mismo snapshot observable.

Una señal opcional puede estar ausente. El engine no la convierte en false, cero
ni un label inventado. Por ejemplo, cara.en_dwell ausente significa que no hay
ROI de dwell configurado; false significa que esa capacidad existe y la
observación fue negativa.

## Semántica de evaluación

Un guard genérico tiene la forma conceptual:

~~~text
(tag, operador, valor)
~~~

Ejemplos:

~~~text
cara.confianza >= 0.80
ocupacion.cardinalidad == "multiple"
~~~

La tabla siguiente resume la regla normativa de la spec:

| Tipo | Operadores admitidos |
|---|---|
| Bool | ==, != |
| Count | ==, !=, >=, <=, >, < |
| Ratio | >=, <=, >, < |
| Label | ==, != |

Ratio sólo representa valores finitos entre 0.0 y 1.0. La igualdad exacta no es
una operación disponible.

### Ausencia

Si un tag conocido está ausente en un tick, cualquier guard genérico sobre ese
tag no coincide, incluido !=. La ausencia no se transforma en false ni se
reporta como error de programa.

Esto distingue dos situaciones clínicamente diferentes:

| Estado de cara.en_dwell | Resultado |
|---|---|
| true | la cara está dentro de un ROI configurado |
| false | el ROI está configurado y la cara está afuera |
| ausente | no hay evidencia de dwell disponible |

La protección frente a datos viejos o cámara sin señal sigue siendo
responsabilidad de Health y sus guards, no una conversión de ausencia a false.

## Estados y fallas

| Situación | Respuesta funcional |
|---|---|
| Configuración inválida en arranque | errores acumulados; el programa no inicia |
| Evidencia fresca | se materializa la tabla y se evalúa normalmente |
| Señal opcional ausente | no se materializa el tag; el guard propuesto no coincide |
| Datos de cámara viejos o ausentes | Health conserva la ruta segura hacia blind |
| Valor interno inválido al producir una señal | `SignalFault`, estado seguro y diagnóstico; nunca coerción silenciosa ni reutilización |
| Falla un sink de reporte | el reporte degrada o pierde el evento; T2 sigue tickeando |

La transición basada en que una zona queda vacía conserva su guard de zona. El
motor de señales no debe reconstruir ese resultado desde un Bool, porque se
perderían la histéresis y los timers que le dan significado clínico.

## Observabilidad del gemelo digital

En la Etapa D, el lote de eventos de un scan incorpora un snapshot completo de
la tabla que se evaluó en ese mismo tick. Esto permite reconstruir:

1. Qué tags estaban presentes y cuáles ausentes.
2. Qué valor tenía cada señal presente.
3. Qué transición evaluó el FSM y cuál fue su resultado.
4. Por qué una condición no coincidió sin inferirlo de logs parciales.

El reporte continúa siendo best-effort. La incorporación es aditiva: el golden
anterior debe mantenerse como prefijo del nuevo. El evento se llama
`SceneEvent::SceneSignals`, usa el `ControlStamp` existente e incluye el
catálogo v1 y las nueve señales; el esquema técnico y la serialización están
cerrados en [design.md](design.md#11-observabilidad-y-gemelo-digital).

## Entrega progresiva

| Etapa | Estado funcional alcanzado |
|---|---|
| A | existen catálogo, tipos, operadores y tabla, sin fuente ni lectura productiva |
| B | el snapshot se materializa en paralelo; el contexto actual aún decide |
| C | guards genéricos leen la tabla después de validación de arranque |
| D | la tabla sustituye el contexto plano y se vuelve observable en el log |

Cada paso depende de que las compuertas del plan demuestren que se preservó el
comportamiento clínico.

## Decisiones cerradas

- El catálogo v1 tiene nueve tags: ocho señales base y
  `cara.estuvo_dentro`, derivada del latch existente.
- Un guard usa `{ type = "signal", tag, op, value }` en TOML y se compila a
  una regla tipada en boot.
- La ausencia nunca coincide, incluido con `!=`.
- La tabla inválida produce `SignalFault` y ruta segura, no un valor por
  defecto.
- D emite `SceneEvent::SceneSignals` con `ControlStamp`, versión de
  catálogo y valores/ausencias ordenados.

## Límites

- No hay tags libres ni reglas nuevas en caliente.
- No se fusionan FsmGuard y ProgramGuard.
- Zonas, Health y profundidad no se migran a Signal.
- El motor de señales no ejecuta modelos, no lee frames y no escribe a sinks.
- Este diseño no altera las decisiones clínicas ni autoriza regenerar goldens
fuera de la adición observacional prevista en D.
