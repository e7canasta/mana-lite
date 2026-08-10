# Señales de escena

**El proyecto abierto.** El refactor por tiers que lo precedió está cerrado y
archivado en [`docs/archive/2026-08-refactor-por-tiers/`](../archive/2026-08-refactor-por-tiers/README.md).

## El problema, en una frase

**Cambiar cuándo suena una alerta clínica es un release.**

Si un servicio pide que la alerta de salida de cama espere 5 segundos en vez de
3, hoy hay que editar Rust, recompilar y desplegar un binario nuevo en el equipo
del cuarto. El número está compilado adentro.

## Qué cambia

| Hoy | Después |
|---|---|
| Un umbral clínico nuevo es un release | Es editar un TOML |
| Todos los servicios comparten la misma respuesta | UTI y sala general pueden tener distinto dwell |
| Ante un incidente, no se puede reconstruir qué vio el sistema | El gemelo digital completo queda en el log |
| Integrarse exige leer nuestro código Rust | Se consume un vocabulario de tags y valores |

El mecanismo: el estado de escena deja de ser un struct de campos con nombre y
pasa a ser una **tabla de señales etiquetadas** que es contrato publicado.

## Por qué ahora

[ADR-031](../adrs/031-scene-signal-table.md) propuso esto hace dos meses y
quedó sin implementar, porque lo justificaba por costo interno de desarrollo
—agregar una regla cuesta 6 ediciones— y lo condicionaba a umbrales de
crecimiento.

Ese encuadre estaba mal. Las 6 ediciones son veinte minutos de una tarea de
días: decidir la semántica clínica, elegir histéresis, escribir tests, validar
contra video real. Optimizarlas no mueve la aguja.

Lo que sí la mueve es que **esto es una interfaz**, y las interfaces tienen otra
ventana. Un refactor interno mal hecho se rehace y nadie se entera; una interfaz
construida después de que existen los consumidores obliga a migrarlos.

[ADR-032](../adrs/032-scene-signals-as-contract.md) lo reencuadra.

## Documentos

| | Qué |
|---|---|
| [1-spec.md](1-spec.md) | **El contrato.** Tipos, tags, semántica, regla de evolución, qué no se convierte |
| [2-sprints.md](2-sprints.md) | **El plan.** Cuatro etapas con compuertas mecánicas |
| [ADR-032](../adrs/032-scene-signals-as-contract.md) | **La decisión** y su costo |

## El caso que ancla todo

El programa que corre hoy es prevención de caídas de cama:
`idle → watching → bed_approaching → bed_alert`. La transición que importa es
`watching → bed_alert` cuando la zona `bed` queda vacía más de 3 segundos.

Ese "3 segundos" y ese "zona bed" son decisiones clínicas. Hoy están dentro del
binario. Todo este trabajo existe para que vivan en configuración auditable.

## Invariante

> El comportamiento clínico no cambia. Los tres goldens quedan byte-idénticos.

Esto mueve **dónde vive** una decisión, no **cuál es** la decisión.
