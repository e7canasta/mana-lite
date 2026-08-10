# Plan de ejecución — Tabla de señales

*Contrato: [1-spec.md](1-spec.md) · Decisión: [ADR-032](../adrs/032-scene-signals-as-contract.md)*

Cuatro etapas. El orden no es negociable por la misma razón que en el proyecto
anterior: **primero la red, después el cambio.** Cada etapa tiene compuerta
mecánica — comandos con salida esperada, no criterio.

**Invariante de todo el proyecto:**

> El comportamiento clínico no cambia. Los tres goldens quedan byte-idénticos
> en las cuatro etapas.

Si un golden se mueve, no se regenera: se arregla el código. La única excepción
es la Etapa D, que **agrega** un evento nuevo — y ahí el golden viejo tiene que
seguir siendo un prefijo del nuevo.

| Etapa | Qué | Riesgo |
|---|---|---|
| **A** | Vocabulario y tipos, sin conectar nada | ninguno |
| **B** | Producir señales en paralelo al contexto actual | bajo |
| **C** | El guard genérico, migrando los 11 | **alto** |
| **D** | Volcado del gemelo y limpieza | medio |

---

## Etapa A — El vocabulario

Define el contrato sin tocar el lazo. Nada lo consume todavía, así que no puede
romper nada.

1. `SignalTag` vía `domain_id!` en `mana-id`, declarado por el crate productor
   ([ADR-030](../adrs/030-shared-mechanism-owned-vocabulary.md)).
2. `SignalValue` con `Bool`, `Count`, `Ratio`, `Label` y su semántica
   (sección 4.2 de la spec).
3. La tabla: `SignalTable`, con lectura por tag.
4. Los operadores y qué tipo admite cada uno.

**Lo que hay que resolver acá y no después:** `Ratio` fuera de `[0,1]` es un
error, y `==` sobre `Ratio` es un error. Si esas dos reglas no están en el tipo
desde el principio, se cuelan comparaciones exactas de flotante en el lazo
clínico.

### Compuerta

```sh
cargo test -p mana-control
cargo clippy --workspace 2>&1 | grep -c 'too many lines'   # → 0
git diff tests/golden/                                      # → vacío
```

- [ ] `SignalValue` rechaza `Ratio` fuera de rango en construcción
- [ ] No existe forma de comparar dos `Ratio` por igualdad
- [ ] Cero consumidores todavía: `grep -rn SignalTable core/mana-control/src` solo encuentra su propio módulo y sus tests

---

## Etapa B — Producir en paralelo

`update_context` empieza a poblar **también** la tabla, sin que nadie la lea.
`FsmSceneContext` sigue siendo la fuente de verdad para los guards.

Suena redundante y es a propósito: permite verificar que la tabla dice lo mismo
que el contexto **antes** de que algo dependa de ella.

Los 11 tags iniciales salen de los 7 campos actuales:

| Campo hoy | Tag | Tipo |
|---|---|---|
| `person_present` | `persona.presente` | `Bool` |
| *(de `raw_person_count`)* | `persona.cantidad` | `Count` |
| `face_present` | `cara.presente` | `Bool` |
| `face_confidence` | `cara.confianza` | `Ratio` |
| `face_in_dwell` | `cara.en_dwell` | `Bool` |
| `at_edge` | `cara.en_borde` | `Bool` |
| `face_model_ran` | `cara.modelo_corrio` | `Bool` |
| `cardinality` | `ocupacion.cardinalidad` | `Label` |

`face_in_dwell` es `Option<bool>` hoy: ausente cuando no hay ROI configurado.
Un tag ausente y un tag en `false` **no son lo mismo** y el contrato tiene que
distinguirlos — si no, un despliegue sin ROI de dwell se comporta como uno con
la cara afuera.

### Compuerta

```sh
cargo test --workspace && cargo test --workspace --release
git diff tests/golden/                                      # → vacío
```

- [ ] Test de paridad: para el escenario de `multi_actor_cycle`, cada tag
      coincide con el campo equivalente en cada tick
- [ ] Nada lee la tabla todavía

---

## Etapa C — El guard genérico

La etapa de riesgo. Acá cambia el comportamiento del FSM.

1. `ProgramGuard::Signal { tag, op, value }` y su evaluación.
2. `FsmGuard::Signal` como cara al TOML.
3. **La validación en arranque** (sección 5 de la spec): los cinco rechazos,
   con mensaje que diga qué tag, qué transición y qué se esperaba.
4. Migrar los 11 guards, **uno por commit**.

### Por qué uno por commit

Son 11 predicados que deciden cuándo suena una alerta clínica. Migrarlos en
lote hace que un golden roto no diga cuál falló. Uno por commit convierte
"algo se rompió" en "se rompió `face_at_edge`".

Los 7 que **no** se migran —zonas, salud, profundidad— se dejan explícitamente,
no por olvido. Ver sección 6 de la spec.

### Compuerta

```sh
cargo test --workspace && cargo test --workspace --release
cargo test --workspace --no-default-features --features ffmpeg
git diff tests/golden/                                      # → vacío
```

- [ ] `FsmGuard` bajó de 18 a 8 variantes
- [ ] Un catálogo con un tag inexistente **falla en boot** con mensaje útil
- [ ] Un catálogo con `==` sobre un `Ratio` **falla en boot**
- [ ] Un catálogo con `>=` sobre un `Bool` **falla en boot**
- [ ] Los tres goldens byte-idénticos

**Antes de confiar en esta compuerta:** escribir los tres catálogos inválidos
de arriba y verificar que fallan. Una validación que nunca se probó contra
entrada inválida no es una validación. Ya nos pasó dos veces.

---

## Etapa D — El gemelo visible

1. Evento de volcado de la tabla completa, emitido con el lote de `scan()`.
2. Retirar `FsmSceneContext` como struct plano.
3. Revisar si `multi_actor_cycle.events.txt` sigue haciendo falta: si el
   volcado deja `Occupancy` y `FsmState` observables en el JSONL, el fixture
   extra pierde razón de ser.

Esta es la única etapa que mueve un golden, porque agrega un evento. El golden
viejo tiene que seguir siendo **prefijo** del nuevo: si además cambió algo, es
que se coló un cambio de comportamiento.

### Compuerta

```sh
cargo test --workspace && cargo test --workspace --release
```

- [ ] El volcado incluye los 11 tags en cada tick
- [ ] El diff del golden es **sólo** líneas agregadas
- [ ] Un incidente se puede reconstruir del log: qué señales había cuando el
      FSM decidió (o no decidió) alertar

---

## Después: lo que esto habilita

No es parte del plan, es lo que el plan hace posible. Se anota para que la
decisión de hacerlo o no sea explícita:

- **Un blueprint puede definir condiciones nuevas** sobre las señales
  existentes sin tocar Rust ([sección 8 de la spec](1-spec.md)).
- **Umbrales por servicio.** UTI y sala general con distinto dwell.
- **Consumidores externos.** Workflows, tableros, gateways que hablan de tags y
  valores.

## Método

El mismo de los seis sprints anteriores, que funcionó: cinco fases —congelar,
ejecutar, verificar, revisar, cerrar—, commits que no mezclan *mover* con
*cambiar*, y compuertas mecánicas.

Y las cuatro lecciones que costaron caro:

1. **Verificá que la compuerta falle cuando debe fallar.** Dos pasaron en vacío
   durante sprints enteros.
2. **La config no debe declarar lo que el código no honra.** Pasó cuatro veces.
3. **El golden JSONL no ve todo.** El logger descarta eventos.
4. **Medí la condición, no un síntoma que la acompaña.**
