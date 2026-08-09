# ADR-030: Shared Mechanism, Owned Vocabulary

**Status:** Accepted
**Date:** 2026-08-09

## Context

`src/domain.rs` define un sistema limpio de newtypes: `DomStr` (newtype sobre
`Arc<str>`) y el macro `domain_id!` que genera `ModelId`, `ClassName`,
`StateId`, `ZoneId` con `Deref`, `Borrow<str>`, `Display` y `Debug`.

Durante la extracción de `mana-control` ese vocabulario **se forkeó mal**:
`core/mana-control/src/domain.rs` son 6 líneas con el macro copiado y
minificado en una sola línea, sin `DomStr` — no compila.

La pregunta de diseño es dónde vive el vocabulario cuando dos crates que no
pueden verse entre sí (ADR-028) necesitan ambos identificadores semánticos.

La respuesta habitual es un crate `mana-core` con los tipos compartidos. Se
rechaza: ese crate se convierte en el vertedero donde ambos lados depositan
tipos "comunes" hasta que el puerto entre ellos es decorativo. Un `ClassName`
compartido adquiere en seis meses un campo que solo le sirve a percepción, y
control ya no puede evolucionarlo.

## Decision

> **Se comparte el mecanismo, no el vocabulario.**

1. `DomStr` y el macro `domain_id!` viven en **`mana-id`** (T0, ~100 líneas).

2. Las **instancias** las declara cada dueño:
   - `mana-perception`: `ModelId`, `ClassName`
   - `mana-control`: `StateId`, `ZoneId`, `ClassName` (la suya)

3. El adaptador del runtime convierte en el puerto. Como ambos lados son
   newtypes sobre `DomStr` con la misma forma, la conversión es un `.as_str()`.

4. Se prohíbe `String` para identificadores semánticos en las APIs públicas de
   T1 y T2. En particular `SceneObservation { class, source_models }` migra de
   `String` a los newtypes correspondientes.

### Por qué el macro sí se comparte

Un macro no puede ser vector de acoplamiento: no arrastra tipos, no tiene
campos, no evoluciona con los requisitos de un lado. Compartirlo cuesta cero
y elimina la duplicación real (el fork actual). Compartir el **tipo** sí
acopla, porque el tipo tiene campos y los campos crecen.

## Consequences

- **Positivo:** Elimina el fork de `domain.rs` sin crear un crate compartido de
  tipos de dominio.
- **Positivo:** Cada lado del puerto puede evolucionar su vocabulario sin
  coordinar con el otro. Eso es lo que un puerto significa.
- **Positivo:** Recupera el type-safety que `domain.rs` ya ofrecía y que las
  APIs públicas anulaban al aceptar `&str`.
- **Negativo:** Conversión explícita en el adaptador. Es el costo del puerto y
  es una línea por campo.
- **Negativo:** Dos tipos llamados `ClassName` en el workspace. Se acepta:
  están en crates que no pueden verse entre sí, así que la ambigüedad no puede
  materializarse en un archivo.

## References

- ADR-027 (tiers), ADR-028 (crate boundaries)
