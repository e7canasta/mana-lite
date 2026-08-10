# Roadmap — Refactorización por Tiers

*Baseline: 2026-08-09.*

Mana Lite es un **PLC cuyo dispositivo de campo resulta ser una cámara**. Este
folder contiene la reorganización del workspace por **clase de determinismo** —
no por tema, ni por dominio de negocio.

## Documentos

| | Documento | Para qué |
|---|---|---|
| **1** | [Big Picture](1-big-picture.md) | Los cuatro tiers, la matriz de dependencias, el estado real medido y cómo crece el diseño. **Normativo.** |
| **2** | [Sprints](2-sprints.md) | Seis sprints con compuertas mecánicas, las cinco fases y el contrato de revisión. **Ejecutable.** |
| **0** | [Onboarding](0-onboarding.md) | **Empezá acá.** Modelo mental, tiers, estado medido, cómo buildear, dónde está la red de seguridad y qué queda abierto. |

## En una pantalla

**La pregunta que ubica cualquier cosa:**

> Si la entrada nunca vuelve a llegar, ¿este componente tiene que seguir
> produciendo salida correcta en cada tick?
> **Sí → T2 programa. No → T1 campo.**

**Los tiers:**

| | Rol | Tasa | Fallo | Crates |
|---|---|---|---|---|
| T0 | álgebra | — | no falla | `mana-id`, `mana-geometry` |
| T1 | campo | variable | **normal** | `mana-media`, `mana-perception` |
| T2 | programa | **fija** | **nunca: tickea siempre** | `mana-control` |
| T3 | reporte | best-effort | silencioso | en el binario |

Entre T1 y T2: `ProcessImage` — congelada, fechada, con edad. Pertenece a T2.

**El enforcement:** un crate se justifica solo si vuelve *imposible de compilar*
una dependencia. La matriz se escribe en los `Cargo.toml`, no en documentación.
La única excepción es el reloj, que se cierra con un tipo
([ADR-029](../adrs/029-injected-clock.md)).

## Estado

El workspace **compila** (0 errores) y la suite **pasa**. La extracción de
`mana-control` y `mana-perception` cerró en `91af4e8`; el Sprint 0 residual
reconectó la evidencia de profundidad al lazo de control y desforkeó `DomStr`.
Siguiente: [Sprint 1](2-sprints.md#sprint-1--red-de-seguridad) — congelar el
comportamiento observable antes de sellar fronteras.

## ADRs

| ADR | Título | Status |
|---|---|---|
| [027](../adrs/027-tier-architecture.md) | Tier Architecture by Determinism Class | Accepted |
| [028](../adrs/028-crate-boundaries.md) | Crate Boundaries as Compile-Time Enforcement | Accepted |
| [029](../adrs/029-injected-clock.md) | Injected Clock in the Program Layer | Accepted |
| [030](../adrs/030-shared-mechanism-owned-vocabulary.md) | Shared Mechanism, Owned Vocabulary | Accepted |
| [031](../adrs/031-scene-signal-table.md) | Scene Signal Table | **Proposed** |

ADR-028 revisa parcialmente [ADR-019](../adrs/019-import-mana-os-std.md). Ninguno
contradice [ADR-001](../adrs/001-single-binary.md) ni
[ADR-003](../adrs/003-plc-superloop.md) — este trabajo es lo que los hace
cumplir.
