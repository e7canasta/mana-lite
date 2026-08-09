# ADR-029: Injected Clock in the Program Layer

**Status:** Accepted
**Date:** 2026-08-09

## Context

ADR-028 hace cumplir la separación de tiers mediante fronteras de crate. Pero
hay un invariante de T2 que **ninguna frontera de crate puede detectar**: el
acceso al reloj de pared. `std::time::Instant` está en la libstd; no hay
`Cargo.toml` que lo prohíba.

El código de control ya está mayormente escrito con reloj inyectado —
`update_at`, `evaluate_at`, `touch_at`, `snapshot_at` reciben el instante como
parámetro. Pero conviven con wrappers de conveniencia que llaman
`Instant::now()` internamente:

- `ScanInstant::now()` — `core/mana-control/src/scan.rs:14`
- `Health::new()`, `Health::touch()`, `Health::evaluate()` — `health.rs:33,48,65`

Cada uno de esos es una puerta al reloj de pared dentro de la capa
determinista. Un solo uso en producción rompe la reproducibilidad del lazo:
el mismo JSONL de entrada deja de producir la misma secuencia de estados.

Es la violación más peligrosa del diseño porque es la única invisible al
compilador y a la revisión de dependencias.

## Decision

**La capa de programa (T2) no lee el reloj. Lo recibe.**

1. `ScanInstant` es construible **únicamente** desde `ScanTimeline`
   (`scan.rs:31`), que ya existe y avanza en múltiplos exactos de
   `scan_period_ms`. Se elimina `ScanInstant::now()`.

2. Se eliminan todos los wrappers sin sufijo `_at` en T2. La variante `_at` es
   la única API pública; el llamador del binario provee el instante.

3. El instante del ciclo se obtiene **una sola vez por tick**, en el runtime
   (T3), y se propaga hacia abajo. Ningún componente de T2 puede observar dos
   instantes distintos dentro del mismo scan.

4. Los tests de T2 construyen su propia `ScanTimeline`. Un test que necesite
   `Instant::now()` está probando el runtime, no el control, y va a `tests/`.

### Por qué un tipo y no una regla

`ScanInstant` sin constructor público equivale a la prohibición: no se puede
fabricar un instante de scan sin una timeline. La disciplina queda expresada
en el sistema de tipos, que es donde sobrevive a la rotación de gente. Una
regla en un documento no sobrevive.

## Consequences

- **Positivo:** El lazo de control es reproducible por construcción. Mismo
  input → misma secuencia de estados, siempre, sin depender de la velocidad de
  la máquina.
- **Positivo:** Los tests de cadencia (`scan_cadence_invariant`,
  `scan_stall_cadence`) dejan de ser sensibles al scheduler del SO.
- **Positivo:** Un revisor externo puede auditar la determinabilidad buscando
  un solo símbolo (`Instant::now`) en un solo crate y esperando cero
  resultados en producción.
- **Negativo:** Las llamadas de conveniencia desaparecen; todo callsite de T2
  debe acarrear el instante. Es más verboso, y es el punto.
- **Negativo:** No cubre otras fuentes de no-determinismo (orden de iteración
  de `HashMap`, NaN en comparaciones de punto flotante). Esas necesitan su
  propia disciplina, fuera del alcance de este ADR.

## Verification

Gate de CI para T2, sin herramienta nueva:

```sh
! grep -rn 'Instant::now\|SystemTime::now\|Utc::now' \
    --include='*.rs' core/mana-control/src \
  | grep -v '#\[cfg(test)\]' 
```

## References

- ADR-027 (tiers), ADR-003 (PLC superloop)
