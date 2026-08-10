# Señales de escena

**El proyecto abierto.** El refactor por tiers que lo precedió está cerrado y
archivado en [`docs/archive/2026-08-refactor-por-tiers/`](../archive/2026-08-refactor-por-tiers/README.md).

## El problema, en una frase

**Expresar una condición clínica nueva sobre evidencia existente es un release.**

El dwell de salida de cama ya está declarado en `config/fsm.toml`. Lo que hoy
requiere tocar Rust es expresar un predicado simple nuevo sobre la evidencia de
escena, porque esa evidencia está acoplada a campos y variantes internas del
FSM.

## Qué cambia

| Hoy | Después |
|---|---|
| Una condición simple nueva es un release | Se expresa sobre señales existentes en TOML |
| La evidencia está acoplada a structs de Rust | El blueprint usa un vocabulario tipado y validado |
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
| [4-big-picture.md](4-big-picture.md) | **La vista general.** Problema, fronteras, evolución y resultado para cada audiencia |
| [1-spec.md](1-spec.md) | **El contrato.** Tipos, tags, semántica, regla de evolución, qué no se convierte |
| [2-sprints.md](2-sprints.md) | **El plan.** Cuatro etapas con compuertas mecánicas |
| [3-sprint-1.md](3-sprint-1.md) | **El primer sprint.** Alcance, decisiones previas, entregables y compuerta de cierre |
| [6-sprint-1-cierre.md](6-sprint-1-cierre.md) | **La evidencia.** Resultados de la compuerta y excepción de línea base |
| [7-sprint-2-handoff.md](7-sprint-2-handoff.md) | **El onboarding.** Entrada operativa para producir señales en paralelo |
| [8-sprint-2-cierre.md](8-sprint-2-cierre.md) | **La evidencia.** Cierre de Etapa B y compuerta de señales en paralelo |
| [9-sprint-3-cierre.md](9-sprint-3-cierre.md) | **La evidencia.** Cierre de Etapa C y migración de guards simples |
| [10-sprint-4-handoff.md](10-sprint-4-handoff.md) | **El onboarding.** Entrada operativa para implementar Etapa D |
| [5-engine-funcional.md](5-engine-funcional.md) | **El engine.** Arranque, tick, evaluación, fallas y observabilidad objetivo |
| [design.md](design.md) | **El diseño técnico.** Decisiones de implementación, catálogo, guard y evento de auditoría |
| [requirements.md](requirements.md) | **Los requisitos.** Resultados de producto, criterios de aceptación y trazabilidad |
| [tasks.md](tasks.md) | **El backlog activo.** Etapas B y C cerradas; Etapa D pendiente |
| [ADR-032](../adrs/032-scene-signals-as-contract.md) | **La decisión** y su costo |

## El caso que ancla todo

El programa que corre hoy es prevención de caídas de cama:
`idle → watching → bed_approaching → bed_alert`. La transición que importa es
`watching → bed_alert` cuando la zona `bed` queda vacía más de 3 segundos.

Ese "3 segundos" y esa zona son decisiones clínicas que ya viven en la
configuración del blueprint. Este trabajo hace auditable y configurable la
evidencia simple adicional con la que una FSM puede componer decisiones.

## Invariante

> El comportamiento clínico no cambia. En A-C los tres goldens quedan
> byte-idénticos; en D el golden anterior es prefijo del nuevo.

Esto mueve **dónde vive** una decisión, no **cuál es** la decisión.
