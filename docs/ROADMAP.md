# Mana Lite — Roadmap

## Proyecto abierto: señales de escena

**Que cambiar una regla clínica deje de ser un release.**

Hoy, si un servicio pide que la alerta de salida de cama espere 5 segundos en
vez de 3, hay que editar Rust, recompilar y desplegar un binario nuevo. El
número está compilado adentro.

- [docs/scene-signals/](scene-signals/README.md) — el problema y qué cambia
- [Spec del contrato](scene-signals/1-spec.md) — tipos, tags, evolución
- [Plan por etapas](scene-signals/2-sprints.md) — cuatro etapas con compuertas
- [ADR-032](adrs/032-scene-signals-as-contract.md) — la decisión y su costo

Estado: **diseñado, sin empezar.** Arranca por la Etapa A (el vocabulario).

## Proyecto cerrado: refactor por tiers

Seis sprints, cerrados 2026-08-10. Archivado en
[docs/archive/2026-08-refactor-por-tiers/](archive/2026-08-refactor-por-tiers/README.md).

Dejó el lazo de control auditable por alguien que no lo escribió: `scan()` en
ocho pasos nombrados, cero funciones de producción sobre 80 líneas, cero
archivos de `src/` sobre 600, reloj sellado por tipo, 366 tests y CI corriendo
la compuerta.

| Sprint | Estado |
|---|---|
| 0 Compilación verde | cerrado |
| 1 Red de seguridad | cerrado |
| 2 Sellar la frontera | cerrado |
| 3 Legibilidad del lazo (A y B) | cerrado |
| 4 Consolidar `std/` + gate `rerun` | cerrado — 6 paquetes |

## Paquetes del workspace

`mana-lite` · `mana-control` · `mana-perception` · `mana-id` · `mana-geometry` · `mana-media`

Mapa de tiers y dependencias: [ARCHITECTURE.md](ARCHITECTURE.md).
Para retomar el trabajo: [HANDOFF.md](../HANDOFF.md).
