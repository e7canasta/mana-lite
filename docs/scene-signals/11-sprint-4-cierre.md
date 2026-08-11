# Cierre — Etapa D: el gemelo visible

**Estado:** Etapa D cerrada
**Entrada:** [10-sprint-4-handoff.md](10-sprint-4-handoff.md)
**Plan:** [2-sprints.md](2-sprints.md#etapa-d--el-gemelo-visible)

## Entrega

- `scan()` emite exactamente un `SceneEvent::SceneSignals` por ciclo, después
  de congelar el snapshot y antes de evaluar la FSM.
- El evento usa el `ControlStamp` del mismo input y conserva la ausencia de
  `cara.estuvo_dentro` cuando no hay `FsmEngine`.
- `Event::SceneSignals` llega al JSONL con nivel informativo, persistencia por
  defecto, `catalog_version = 1` y las nueve entradas ordenadas por tag.
- Cada entrada incluye su `kind` y un `value` o `absent: true`.
- `ControlState`, App y `FaceDwellLogStrategy` ya no mantienen ni consumen el
  contexto plano para la ruta productiva. El `FsmEngine` conserva adaptadores
  de contexto para las APIs de prueba existentes; su evaluación productiva
  recibe sólo el snapshot.
- El logger continúa fuera de T2. Un handler degradado no impide que otro
  destino reciba el evento.

## Evidencia

- `cargo test -p mana-control -p mana-lite --lib`: correcto.
- `cargo test --tests`: correcto.
- `cargo test --workspace --release`: correcto.
- `cargo test --workspace --no-default-features --features ffmpeg`: correcto.
- `cargo fmt --all`: correcto.
- `git diff --check`: correcto.
- Los goldens de `multi_actor_cycle` y `synthetic_cycle` sólo agregan líneas de
  `scene_signals`; las transiciones y los eventos legacy conservan su contenido.
- `multi_actor_cycle.events.txt` verifica el orden bruto y un único evento por
  scan; sus aserciones verifican sello, nueve tags, valores y ausencias.

## Decisiones cerradas

- El wire format usa un array ordenado, no un objeto JSON basado en `HashMap`.
- Ratio se serializa mediante su API pública de lectura, sin exponer
  construcción inválida ni igualdad clínica.
- `FaceDwell` legado permanece como evento diagnóstico compatible; sus campos
  de escena ahora proceden de `SceneSignalsSnapshot`.
- El toggle `scene_signals_events` existe para perfiles explícitos, pero su
  valor por defecto es `true` y el nivel mínimo es informativo.

## Deuda conocida

La compuerta estricta de Clippy mantiene la deuda de línea base documentada en
las etapas anteriores. No se introducen filtros globales ni se mezcla esa
limpieza con el contrato de observabilidad.

La operación de blueprints está en [12-blueprint-onboarding.md](12-blueprint-onboarding.md);
la deuda y las decisiones acumuladas están en [deuda.md](deuda.md) y
[adrs.md](adrs.md).
