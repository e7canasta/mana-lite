# Archivo — Refactor por tiers (2026-08)

**Estado: cerrado.** Este proyecto terminó. Se archiva completo porque su
razonamiento sigue siendo útil, pero **ya no describe trabajo pendiente**.

Para el estado actual del sistema y lo que sigue, ver
[`docs/scene-signals/`](../../scene-signals/README.md).

## Qué cambió para el producto

Antes de este proyecto, el lazo de control clínico **no se podía auditar por
nadie que no lo hubiera escrito**: `scan()` era una función de una línea de
3.686 caracteres, la lógica de percepción leía estado del programa, y el reloj
se tomaba de la pared en pleno tier determinista.

Eso bloqueaba tres cosas concretas del negocio:

- **Certificación.** Ningún revisor externo —una fundación, un auditor
  clínico— podía leer el lazo que decide si suena una alerta.
- **Portabilidad a mana-os.** La lógica clínica estaba enredada con la captura,
  así que no se podía mover a un host multi-cámara.
- **Confianza en los cambios.** Con 5 tests de integración, tocar una regla era
  apostar.

Hoy: el lazo son ocho pasos nombrados, ningún archivo pasa de 600 líneas,
ninguna función de producción pasa de 80, el reloj está sellado por tipo, y hay
366 tests con tres fixtures que congelan el comportamiento observable.

## Lo que se descubrió por el camino

Cuatro hallazgos que no estaban en ningún plan y que valen para cualquier
proyecto que siga:

**La config declaraba cosas que el código no honraba.** Tres veces: un
parámetro de validación de modelos que no validaba nada, un `min_confidence`
en `zone_vacated` que no podía evaluarse, y un feature `rerun` que no apagaba
nada. Un knob que se acepta y se ignora es peor que uno que no existe: miente
en la revisión.

**Las compuertas pueden pasar en vacío.** `git diff tests/golden/` no verificó
nada durante dos sprints porque el fixture estaba sin trackear. Antes de
confiar en una compuerta, hay que verificar que **falle cuando debe fallar**.

**El golden JSONL no ve todo.** El logger descarta dos tipos de evento, así que
reordenarlos deja el golden verde. Por eso existe
`tests/golden/multi_actor_cycle.events.txt`.

**Un warning no prueba ausencia de `cfg`.** Se concluyó que el gating de
`rerun` faltaba mirando warnings que aparecen igual con el feature encendido.
Medir la condición, no un síntoma que la acompaña.

## Contenido

| Documento | Qué es |
|---|---|
| [0-onboarding.md](0-onboarding.md) | El modelo mental: PLC, tiers, la pregunta de pertenencia |
| [1-big-picture.md](1-big-picture.md) | Arquitectura por tiers, matriz de dependencias |
| [2-sprints.md](2-sprints.md) | Los seis sprints con sus compuertas y el método de revisión |

Los ADRs que salieron de ací siguen vigentes y **no** se archivan:
[027](../../adrs/027-tier-architecture.md) (tiers),
[028](../../adrs/028-crate-boundaries.md) (fronteras de crate),
[029](../../adrs/029-injected-clock.md) (reloj inyectado),
[030](../../adrs/030-shared-mechanism-owned-vocabulary.md) (vocabulario).
