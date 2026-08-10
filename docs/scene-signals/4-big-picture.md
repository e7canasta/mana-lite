# Big picture — Señales de escena

*Contrato: [1-spec.md](1-spec.md) · Plan: [2-sprints.md](2-sprints.md) · Diseño funcional: [5-engine-funcional.md](5-engine-funcional.md) · Diseño técnico: [design.md](design.md) · Decisión: [ADR-032](../adrs/032-scene-signals-as-contract.md)*

## La decisión en una frase

La escena deja de ser un conjunto privado de campos dentro del proceso y pasa a
ser una **tabla de señales tipadas, declaradas y versionadas**. El cambio no
modifica qué decisión clínica toma el sistema; convierte en contrato la
evidencia con la que la toma.

Hoy, cambiar una condición clínica exige editar Rust, recompilar y desplegar.
Cuando las etapas estén completas, un blueprint podrá expresar otra condición
sobre señales ya producidas sin cambiar el binario.

## El problema que resuelve

La prevención de caídas de cama funciona con un programa que recorre
idle → watching → bed_approaching → bed_alert. La alerta se dispara por una
decisión clínica concreta: por ejemplo, que la zona bed quede vacía durante un
dwell configurado.

El problema no es que existan guards ni que el FSM sea complejo. El problema es
que parte de la evidencia que usan esos guards vive como detalle interno:

- Un servicio no puede variar una condición existente sin un release.
- Ante un incidente, el log no permite reconstruir toda la escena que vio el
  programa.
- Un consumidor externo tendría que conocer los campos y enums de Rust para
  interpretar la evidencia.

Las señales cambian esas tres cosas al establecer un vocabulario público. No
convierten la visión por computadora en un motor de reglas ni reemplazan los
componentes que ya poseen lógica clínica real.

## El modelo mental

mana-lite es un PLC cuyo dispositivo de campo es una cámara. El campo produce
evidencia a una tasa variable; el programa de control decide a cadencia fija,
incluso si el campo dejó de entregar datos.

~~~text
Campo: cámara, decode, modelos
             │ evidencia con edad
             ▼
Imagen de proceso ──► programa de control por tick
                         │
                         ├── presencia, ocupación, zonas, salud y profundidad
                         │
                         ├── tabla de señales: tag → valor
                         │        │
                         │        ├── guards genéricos del FSM
                         │        └── consumidores externos, más adelante
                         │
                         └── transición, estado y eventos de escena
                                      │
                                      ▼
                              reporte best-effort: JSONL, métricas, etc.
~~~

La tabla es un snapshot de la escena en un tick. No es una cola de eventos, una
base de datos, un cache entre ticks ni un canal de configuración en caliente.

## Qué pertenece a la tabla y qué no

Una señal representa evidencia que se puede expresar como una comparación sobre
un valor tipado: presencia de persona, cantidad, confianza de cara, estado de
dwell, borde y cardinalidad son los candidatos iniciales.

Los guards que necesitan un motor o estado propio no se fuerzan dentro de la
tabla:

| Responsabilidad | Continúa en su componente |
|---|---|
| Histéresis y timers espaciales | motor de zonas |
| Cámara sin señal o evidencia vieja | Health |
| Reglas contra el mapa de profundidad | evaluador de profundidad |

Por ejemplo, la transición clínica que depende de que la zona bed quede vacía
continúa usando el guard de zonas. La tabla habilita comparaciones sobre
evidencia de escena; no borra la semántica espacial ni sus timers.

## El contrato como frontera

Cada señal tiene un tag, un tipo y una semántica estable:

| Ejemplo | Tipo | Lectura externa |
|---|---|---|
| persona.presente | Bool | hay o no hay persona |
| persona.cantidad | Count | cuántas personas hay |
| cara.confianza | Ratio | proporción de 0.0 a 1.0 |
| ocupacion.cardinalidad | Label | valor dentro de un conjunto conocido |

El productor declara el vocabulario; no hay tags libres. Un tag puede agregarse
de forma compatible, pero quitarlo, renombrarlo o cambiar su tipo o rango rompe
el contrato y requiere un tag nuevo con convivencia.

La ausencia también es parte del contrato. Una señal ausente no significa
false: puede indicar que una capacidad no está configurada o que esa evidencia
no existe para el tick. Esa distinción evita que un despliegue sin ROI de dwell
se comporte como uno que midió una cara fuera del ROI.

## Dos momentos, dos garantías

La propuesta conserva la separación entre texto de configuración y programa
ejecutable:

~~~text
En arranque
blueprint + catálogo de señales + catálogos de referencia
                       │
                       ▼
         validar y compilar el programa del FSM
                       │
                       ▼
          programa fijo para toda la ejecución

En cada tick
evidencia actual + programa ya compilado
                       │
                       ▼
      snapshot de señales + evaluación determinista
~~~

El arranque rechaza un tag inexistente, un operador incompatible, una igualdad
de Ratio, un valor fuera de rango o un Label que el productor no puede emitir.
El tick sólo ejecuta un programa ya validado; no interpreta reglas nuevas ni
recarga configuración.

## Qué habilita cada etapa

| Etapa | Capacidad nueva | Lo que deliberadamente no cambia |
|---|---|---|
| A | vocabulario, tipos, tabla y operadores | el lazo no produce ni consume señales |
| B | tabla producida en paralelo al contexto actual | los guards siguen leyendo el contexto actual |
| C | condiciones genéricas configurables sobre señales | zonas, salud y profundidad conservan su lógica |
| D | snapshot completo observable por tick | la decisión clínica previa sigue siendo la misma |

El orden protege el comportamiento clínico: primero se define el contrato,
después se contrasta la tabla contra la fuente actual, luego se migra la lectura
y por último se expone el snapshot al reporte.

## Resultado para cada audiencia

| Audiencia | Resultado útil |
|---|---|
| Administración clínica | variar condiciones existentes por blueprint, con validación de arranque |
| Operación | reconstruir qué señales había cuando se tomó o no una decisión |
| Desarrollo | agregar evidencia al contrato sin crear una variante de guard por cada comparación |
| Integraciones | consumir tags y valores sin conocer estructuras internas de Rust |

## Límites que se preservan

- No hay reglas nuevas en caliente: cambiar configuración implica un arranque
  validado del programa.
- No se fusionan FsmGuard y ProgramGuard: uno es el texto del programa y el otro
  la representación compilada para el tick.
- La tabla no reemplaza inferencia, zonas, salud ni profundidad.
- El reporte es best-effort y nunca bloquea el lazo de control.
- Hasta la Etapa D, los tres goldens permanecen byte-idénticos. En D sólo se
  permiten líneas de observabilidad agregadas.

## Criterio de éxito

El trabajo estará completo cuando el sistema pueda responder, a partir del log,
qué señales vio en cada decisión; y cuando un blueprint pueda expresar una
condición clínica nueva sobre señales existentes sin modificar Rust. Ambos
resultados deben conservar el mismo comportamiento observable del programa
clínico actual.
