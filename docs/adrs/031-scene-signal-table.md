# ADR-031: Scene Signal Table

**Status:** Superseded por [ADR-032](032-scene-signals-as-contract.md)
**Date:** 2026-08-09

> Este ADR justificó la tabla de señales por **costo interno de desarrollo** (6
> ediciones por regla) y la condicionó a umbrales de crecimiento. Ese encuadre
> era incorrecto y por eso quedó dos meses sin implementar: las 6 ediciones son
> veinte minutos de una tarea de días, y esperar al umbral no aplica a una
> interfaz. ADR-032 la reencuadra como **contrato publicado** y agrega las
> reglas que este documento no tenía: vocabulario declarado, semántica de
> valores, regla de evolución y observabilidad del gemelo completo.
>
> El análisis técnico de abajo sigue siendo válido y es la base de ADR-032.

## Context

Extender la lógica de escena es la operación más frecuente del sistema y la
que más va a crecer. Hoy agregar un predicado de escena cuesta **6 ediciones
en 4 archivos**, todas dentro de `mana-control`:

| # | Archivo | Edición |
|---|---|---|
| 1 | `fsm/engine.rs:17` | +1 campo en `FsmSceneContext` |
| 2 | `scan.rs` `update_context` | +1 línea que lo puebla |
| 3 | `fsm/guard.rs:21` | +1 variante en `FsmGuard` (cara a TOML) |
| 4 | `fsm/program.rs:97` | +1 variante en `ProgramGuard` (compilada) |
| 5 | `program.rs` `compile()` | +1 brazo de mapeo |
| 6 | `guard.rs` eval | +1 brazo de evaluación |

Que todo caiga en un solo crate confirma que las fronteras de ADR-028 son
correctas. Pero el costo crece lineal y permanente: `FsmGuard` y `ProgramGuard`
tienen hoy **18 variantes cada uno**, sincronizadas a mano. A 40 guards duele;
a 80 es un pasivo. `FsmSceneContext` son 7 booleanos planos y cada predicado
nuevo es un campo más, para siempre.

## Decision

**Se preserva el split `FsmGuard` / `ProgramGuard`.** Esa duplicación aparente
no es deuda: es la separación **compilar en boot / ejecutar determinista** de
un PLC. `FsmGuard` es el texto del programa; `ProgramGuard` es el programa
compilado y resuelto contra los catálogos. Fusionarlos para ahorrar tipeo
destruiría la validación en arranque.

El cambio va un nivel más adentro:

> La imagen de proceso de un PLC no es un struct de booleanos con nombre.
> Es una **tabla de señales etiquetadas**.

1. `FsmSceneContext` pasa de struct plano a tabla de señales tipadas
   (`tag → SignalValue`, donde `SignalValue` cubre `Bool`, `Count`, `Ratio`,
   `Label`).

2. Se agrega una variante genérica `ProgramGuard::Signal { tag, op, value }`
   que cubre la clase mayoritaria de predicados sin variantes nuevas.

3. Agregar una regla de escena pasa a ser **registrar un productor de señal**:
   pasos 1-2 de la tabla. Los pasos 3-6 desaparecen para todo predicado que
   encaje en la forma genérica.

4. Los guards con semántica propia (`ZoneEntered`, `DepthRule`, dwell) siguen
   siendo variantes explícitas. La tabla no los reemplaza.

### Dónde se verifica

Se pierde la exhaustividad del `match` del compilador. La mitigación ya existe:
`FsmProgram::compile(catalog, zones) -> Result<Self, Vec<String>>` valida
contra los catálogos y acumula errores. Un tag inexistente o un tipo incorrecto
falla **al compilar el programa en boot**, no en runtime.

Eso es exactamente el modelo PLC y es aceptable porque el programa es fijo tras
el arranque: el sistema no acepta reglas nuevas en caliente.

## Status: Proposed, no Accepted

Esta decisión **no se implementa hasta cerrar Sprint 3**. Se registra ahora
porque cambia una decisión inmediata: cada booleano plano que se agregue a
`FsmSceneContext` mientras tanto es deuda que habrá que migrar. Si un sprint
intermedio necesita un predicado nuevo, agregarlo con la forma actual es
correcto — pero conscientemente.

Se promueve a Accepted cuando se cumpla alguna condición de disparo:

- `FsmGuard` supera **25 variantes**, o
- `FsmSceneContext` supera **12 campos**, o
- aparece un requisito de reglas de escena definidas por despliegue.

## Consequences

- **Positivo:** El costo de una regla nueva pasa de O(6 ediciones) a O(1-2).
- **Positivo:** Habilita reglas por despliegue sin recompilar el binario.
- **Positivo:** La tabla de señales es directamente inspeccionable — se puede
  volcar el estado completo del gemelo digital en un evento de diagnóstico.
- **Negativo:** Se pierde el chequeo exhaustivo del compilador sobre los
  predicados genéricos; se traslada a `compile()`.
- **Negativo:** Errores de tipeo en tags de TOML fallan en arranque, no en
  compilación de Rust. Se mitiga con `compile()` acumulando todos los errores
  y con un test que compile todos los catálogos del repo.

## References

- ADR-015 (FSM engine), ADR-002 (TOML catalog pattern), ADR-027 (tiers)
