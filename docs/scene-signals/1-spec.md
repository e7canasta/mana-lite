# Spec — Tabla de señales de escena

**Estado:** Diseño aprobado; Etapas A, B y C implementadas; Etapa D abierta
**Version:** 0.1
**Fecha:** 2026-08-10
**Decisión de base:** [ADR-032](../adrs/032-scene-signals-as-contract.md)
**Audiencia:** administradores, operadores, funcionales y desarrolladores

## 1. Objetivo de negocio

Que **expresar una condición clínica nueva sobre evidencia ya producida deje de
ser un release**.

El dwell de salida de cama ya se declara en el blueprint. Pero si un servicio
necesita componer una condición nueva sobre confianza facial, presencia o
cardinalidad, hoy hay que agregar un campo o una variante de guard en Rust,
recompilar y desplegar un binario nuevo en el equipo del cuarto. Después de
esto, la condición se expresa contra el vocabulario declarado en configuración.

Y que el sistema pueda **responder qué vio**. Hoy, ante un incidente, la
pregunta "¿el sistema detectó a la persona antes de la caída?" no tiene
respuesta completa: el estado de la escena vive dentro del proceso y solo salen
al log los eventos que alguien eligió a mano.

## 2. El caso que ancla todo

El programa clínico que corre hoy (`config/fsm.toml`) es **prevención de caídas
de cama**:

| Estado | Significado clínico |
|---|---|
| `idle` | Habitación vacía |
| `watching` | Hay una persona |
| `bed_approaching` | Se acercó al borde de la cama (regla de profundidad) |
| `bed_alert` | Intento de salida de cama — **esto es lo que dispara al personal** |
| `blind` | Sin señal de cámara: estado seguro |

La transición que importa: `watching → bed_alert` cuando la zona `bed` queda
vacía por más de 3 segundos. Ese "3 segundos" y esa zona son decisiones
clínicas ya declaradas en la configuración del blueprint.

Todo lo que sigue existe para que la evidencia simple adicional que compone
esas decisiones tenga un contrato configurable y auditable.

## 3. Alcance y no alcance

### Dentro

- El estado de escena (`FsmSceneContext`) pasa a ser tabla de señales.
- Una variante genérica de guard que compare señales.
- Vocabulario de tags declarado y versionado.
- Un evento de volcado del gemelo digital completo.
- Validación en arranque de tags, tipos y operadores.

### Fuera

- **No** se fusionan `FsmGuard` y `ProgramGuard`. Esa duplicación es la
  separación compilar-en-boot / ejecutar-determinista de un PLC.
- **No** se reemplazan los guards con lógica propia (zonas, salud,
  profundidad). Ver sección 6.
- **No** se aceptan reglas nuevas en caliente. El programa es fijo tras el
  arranque; eso es lo que hace determinista al lazo.
- **No** cambia el comportamiento clínico. Este trabajo preserva la salida
  actual byte a byte.

## 4. El contrato

### 4.1 Señal

Una señal es un par `tag → valor`, producido por el lazo de control en cada
tick y consumido por los guards del FSM y por cualquier sistema externo.

```
persona.presente      = Bool(true)
persona.cantidad      = Count(2)
cara.confianza        = Ratio(0.87)
cara.en_dwell         = Bool(true)
ocupacion.cardinalidad = Label("multiple")
```

### 4.2 Tipos de valor y su semántica

El tipo no alcanza: un consumidor externo tiene que poder interpretar sin leer
nuestro código.

| Tipo | Semántica | Operadores válidos |
|---|---|---|
| `Bool` | verdadero / falso | `==`, `!=` |
| `Count` | cuántos, entero ≥ 0 | `==`, `!=`, `>=`, `<=`, `>`, `<` |
| `Ratio` | proporción de 0.0 a 1.0 | `>=`, `<=`, `>`, `<` |
| `Label` | valor de un conjunto cerrado y conocido | `==`, `!=` |

Dos reglas que se validan en arranque:

- **`Ratio` fuera de `[0,1]` es un error de programa**, no un valor raro. Una
  confianza de 1.5 significa que alguien se equivocó de unidad.
- **Comparar `Ratio` con `==` es un error.** Comparación exacta de flotantes en
  un lazo de control es un bug esperando. El compilador de programa lo rechaza.

### 4.3 Tags

Los tags son **vocabulario declarado**, no strings libres. Se usa el mecanismo
de [ADR-030](../adrs/030-shared-mechanism-owned-vocabulary.md): `domain_id!` de
`mana-id`, con el crate productor declarando sus tags.

Convención de nombre: `dominio.atributo`, minúsculas, guión bajo dentro del
atributo. El dominio agrupa por origen de la evidencia (`persona`, `cara`,
`ocupacion`, `zona`, `profundidad`).

Sin esto, en seis meses conviven `face_in_dwell` y `faceInDwell` y el contrato
no significa nada.

### 4.4 Regla de evolución

Un contrato necesita saber qué se puede cambiar sin romper a quien lo consume:

| Cambio | ¿Compatible? |
|---|---|
| Agregar un tag nuevo | **Sí** |
| Agregar una variante a un `Label` | **Sí**, si los consumidores tratan lo desconocido como "no coincide" |
| Quitar un tag | **No** |
| Renombrar un tag | **No** |
| Cambiar el tipo de un tag | **No** |
| Cambiar el rango semántico (ej. `Ratio` que pasa a ser porcentaje 0-100) | **No** |

Los cambios incompatibles requieren tag nuevo y período de convivencia.

## 5. Dónde se verifica

Se pierde el chequeo exhaustivo del compilador de Rust sobre los predicados
genéricos. **Eso se compensa o la decisión no se sostiene.**

`FsmProgram::compile_with_references(...)` ya existe, acumula errores y corre
en boot. Obtendrá el catálogo estático de señales en el crate productor y tiene
que rechazar:

1. Un tag que ningún productor declara.
2. Un operador que no aplica al tipo del tag (`>=` sobre un `Bool`).
3. Un `==` sobre un `Ratio`.
4. Un valor fuera del rango semántico del tipo.
5. Un `Label` comparado contra un valor que ningún productor puede emitir.

Los cinco con mensaje que diga **qué tag, qué transición y qué se esperaba**.
El test `fsm_catalogs_compile` ya compila todos los catálogos del repo: es
donde esto se prueba.

Esto es el modelo PLC: un programa de ladder logic tampoco se type-checkea en
C, se valida al cargarlo en el controlador.

## 6. Qué NO se convierte en señal

De los 18 guards actuales, 7 conservan variante propia porque su lógica no es
una comparación:

| Guard | Por qué se queda |
|---|---|
| `zone_present`, `zone_occupied`, `zone_vacated`, `all_zones_vacant` | Necesitan el motor de zonas y sus timers de histéresis |
| `data_stale`, `data_fresh` | Leen `Health`, que tiene su propia máquina de estados |
| `depth_rule` | Lee el snapshot de reglas de profundidad, evaluadas contra el mapa |

Los otros 11 —los 8 de cara, los 2 de persona y `cardinality`— son "leé un
campo y comparalo" y pasan a la variante genérica.

## 7. Observabilidad del gemelo digital

Un evento nuevo vuelca la tabla completa, emitido con el resto del lote de
`scan()`.

Esto cierra un agujero que existe hoy: `scene_events_to_log` **descarta**
`SceneEvent::Occupancy` y `SceneEvent::FsmState`, así que reordenarlos deja el
golden JSONL verde. Por eso hay que mantener un fixture aparte
(`tests/golden/multi_actor_cycle.events.txt`) sólo para detectar eso.

Con el volcado de señales, el estado que hoy se pierde queda en el log. Para
una revisión de incidente eso es la diferencia entre "el sistema no alertó" y
"el sistema no alertó **porque** la confianza de cara estaba en 0.31".

## 8. Relación con blueprints

Los blueprints (`config/blueprints/`, [ADR-026](../adrs/026-inference-blueprints.md))
ya son el mecanismo para variar el sistema por despliegue: cada uno trae su
`models.toml` y su `fsm.toml`.

Hoy un blueprint puede cambiar **qué modelos corren** y **qué transiciones
existen**, pero los predicados disponibles están fijos en el binario. Con la
tabla de señales, un blueprint puede además expresar condiciones nuevas sobre
las señales que ya se producen, sin tocar Rust.

Ese es el punto donde el trabajo se vuelve visible para el negocio: un servicio
con una necesidad distinta es un blueprint nuevo, no una versión nueva.

## 9. Invariante

> El comportamiento clínico observable no cambia. En A-C los tres goldens
> quedan byte-idénticos; en D el golden anterior es prefijo del nuevo.

Este trabajo mueve dónde vive una decisión, no cuál es la decisión.
