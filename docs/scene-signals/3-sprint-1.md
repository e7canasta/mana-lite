# Plan detallado — Sprint 1: vocabulario de señales

**Estado:** cerrado funcionalmente; Etapa B abierta

*Contrato: [1-spec.md](1-spec.md) · Plan general: [2-sprints.md](2-sprints.md) · Diseño técnico: [design.md](design.md) · Decisión: [ADR-032](../adrs/032-scene-signals-as-contract.md)*

## Propósito

Este sprint implementa la **Etapa A** del plan general: el contrato base de
señales. Al cerrarlo existe un vocabulario declarado y versionado, valores con
semántica verificable, operadores tipados y una tabla de lectura.

No se conecta nada al lazo de control. Por eso el comportamiento clínico y los
goldens deben permanecer byte-idénticos.

## Resultado esperado

El crate productor expone:

- `SignalTag`, declarado con `domain_id!`; el mecanismo vive en `mana-id`, pero
  el vocabulario pertenece a su dueño.
- Un catálogo versionado con la definición de cada tag: tipo, semántica y, para
  `Label`, el conjunto de valores que puede emitir.
- `SignalValue::{Bool, Count, Ratio, Label}`.
- `SignalOp`, con la matriz de operadores válidos de la spec.
- `SignalTable`, con lectura por tag, ausencia distinguible de `Bool(false)` e
  iteración determinista.

Todavía no hay productores ni consumidores de `SignalTable` fuera de su módulo
y sus pruebas.

## Decisiones cerradas

### Catálogo inicial

El catálogo v1 tiene nueve tags: ocho señales base y
`cara.estuvo_dentro`, que publica el latch existente de `FsmEngine`.
Las once son los guards simples que se reemplazarán en C. La lista canónica,
sus tipos, presencia y labels permitidos están en [design.md](design.md#6-catálogo-v1).

### Ausencia, tipo y evolución

- Una señal ausente no se materializa como un valor falso ni como un valor por
  defecto. Es parte del contrato para señales opcionales, en particular
  `cara.en_dwell`.
- `Count` usa un entero no negativo de ancho fijo en el contrato público; no un
  tipo dependiente de la arquitectura.
- Quitar, renombrar o cambiar el tipo/rango de un tag exige un tag nuevo y un
  período de convivencia, según la spec.
- El catálogo valida pertenencia: envolver un string en `SignalTag` no basta
  para convertirlo en vocabulario declarado.

### Ratio y comparación

`Ratio` es opaco y sólo se construye si su valor es finito y pertenece a
`[0.0, 1.0]`. No implementa ni expone una API que permita igualdad exacta entre
ratios. Sus únicos operadores son `>=`, `<=`, `>` y `<`.

## Plan de ejecución

### Día 1 — Congelar contrato y pruebas de borde

1. Verificar la lista canónica de nueve tags contra
   [design.md](design.md#6-catalogo-v1) y mantener la distinción con los once
   guards de C.
2. Aplicar el catálogo v1: versión entera `1` y labels cerrados por tag.
3. Escribir la matriz de pruebas: tipo-operador, valores de borde, ausencia y
   tags/labels desconocidos.
4. Acordar la prueba de compilación que demuestra que dos `Ratio` no se pueden
   comparar por igualdad.

**Salida:** contrato sin ambigüedades y casos de prueba aprobados antes de
introducir la API.

### Día 2 — Vocabulario y catálogo

1. Declarar `SignalTag` con `domain_id!` en `mana-control`, el crate dueño
   del vocabulario.
2. Implementar los descriptores del catálogo: versión, tipo y labels válidos.
3. Validar la convención `dominio.atributo`, los tags declarados y los labels
   cerrados.
4. Cubrir tag conocido, tag desconocido, tag con nombre inválido y label fuera
   del catálogo.

**Salida:** el vocabulario es una interfaz explícita, no un conjunto de
strings aceptados por convención.

### Día 3 — Valores y operadores

1. Implementar `SignalValue` y `SignalOp`.
2. Encapsular la construcción de `Ratio`; rechazar valores menores que cero,
   mayores que uno, `NaN` e infinitos.
3. Implementar la matriz de compatibilidad:

| Tipo | Operadores admitidos |
|---|---|
| `Bool` | `==`, `!=` |
| `Count` | `==`, `!=`, `>=`, `<=`, `>`, `<` |
| `Ratio` | `>=`, `<=`, `>`, `<` |
| `Label` | `==`, `!=` |

4. Añadir una prueba de compilación para impedir igualdad de ratios y pruebas
   unitarias de todos los bordes semánticos.

**Salida:** los invariantes clínicos viven en el tipo, no en una convención de
quien consume el valor.

### Día 4 — Tabla de señales

1. Implementar `SignalTable` como estructura ordenada para que su futura
   observabilidad sea determinista.
2. Exponer lectura por tag e iteración; mantener la diferencia entre ausencia
   y valor presente.
3. Al escribir, comprobar que el tag pertenece al catálogo y que el valor
   coincide con su descriptor.
4. Probar orden estable, lecturas, ausencia, tipo incorrecto y label inválido.

**Salida:** una tabla autocontenida y verificable, todavía sin conexión con
`FsmSceneContext`, `scan()` ni guards.

### Día 5 — Compuerta, revisión y cierre

1. Ejecutar la compuerta mecánica y conservar su salida literal.
2. Verificar que los goldens no cambiaron y que no apareció un consumidor
   accidental de la tabla.
3. Revisar el diff contra contrato, no sólo contra compilación: semántica de
   `Ratio`, ausencia y versión del vocabulario.
4. Cerrar con la lista de decisiones tomadas y cualquier desvío del plan.

## Entregables

- Catálogo inicial de señales, versionado y documentado.
- Tipos `SignalTag`, `SignalValue`, `SignalOp` y `SignalTable`.
- Pruebas unitarias y de compilación del contrato.
- Catálogo canónico de nueve tags y documentación consistente con los once
  guards a migrar.
- Salida literal de la compuerta de cierre.

## Compuerta de cierre

```sh
cargo test -p mana-control
cargo clippy --workspace -- -D warnings
git diff tests/golden/                                      # vacío
grep -rn SignalTable core/mana-control/src
```

El último comando sólo puede encontrar el módulo de señales y sus pruebas.

Además, deben estar demostrados:

- `Ratio(0.0)` y `Ratio(1.0)` son válidos.
- Ratios negativos, mayores que uno, no finitos o `NaN` fallan al construirlos.
- No existe igualdad exacta entre ratios.
- Cada combinación inválida de tipo y operador falla de forma explícita.
- Un tag o label no declarado no entra en la tabla.
- No cambia ningún golden.

La compuerta estricta de Clippy se ejecutó y queda con excepción de línea base:
el workspace tiene warnings preexistentes en módulos ajenos a `signals/`. La
evidencia completa está en [6-sprint-1-cierre.md](6-sprint-1-cierre.md).

## Fuera de alcance

Este sprint no incluye:

- Poblar la tabla desde `update_context` o cualquier otro paso de `scan()`.
- Reemplazar o modificar `FsmSceneContext`.
- Agregar `FsmGuard::Signal` o `ProgramGuard::Signal`.
- Cambiar la sintaxis de `fsm.toml` o la validación de programas en boot.
- Migrar los 11 guards simples ni tocar los siete guards que conservan lógica
  propia de zonas, salud o profundidad.
- Emitir el volcado del gemelo digital, cambiar el logger o regenerar goldens.
- Reglas nuevas en caliente, cambios de blueprints o cambios de umbrales
  clínicos.

Esos trabajos pertenecen, respectivamente, a las Etapas B, C y D. El criterio
de salida del sprint es haber preparado el contrato sin alterar el programa que
corre hoy.
