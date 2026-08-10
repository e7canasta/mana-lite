# Cierre — Etapa C: el guard genérico

**Estado:** Etapa C cerrada; Etapa D pendiente
**Entrada:** [8-sprint-2-cierre.md](8-sprint-2-cierre.md)
**Plan:** [2-sprints.md](2-sprints.md#etapa-c--el-guard-genérico)

## Entrega

- `FsmGuard` quedó en ocho variantes: siete guards especializados y `Signal`.
- `ProgramGuard::Signal` conserva `SignalTag`, `SignalOp` y `SignalValue` ya tipados.
- `SignalLiteral` permite deserializar booleanos, enteros, floats y texto sin
  ocultar errores de configuración durante la carga TOML.
- `FsmProgram::compile_with_references()` valida en boot el catálogo estático,
  el operador, el literal, los rangos, los labels y la restricción de wildcard.
- Los 11 guards simples fueron migrados a expresiones sobre las nueve señales:
  presencia, confianza, dwell, borde, latch y cardinalidad.
- La evaluación productiva usa un único `SceneSignalsSnapshot` congelado por
  ciclo; zonas, Health y profundidad conservan sus evaluadores propios.
- Los cinco rechazos normativos y los casos adicionales de operador desconocido,
  count negativo, ratio no finito/fuera de rango y label no emitible tienen
  pruebas de boot.

## Evidencia

- `cargo test --workspace`: correcto.
- `cargo test --workspace --release`: correcto.
- `cargo test --workspace --no-default-features --features ffmpeg`: correcto.
- `cargo fmt --all -- --check`: correcto.
- `git diff --check`: correcto.
- `git diff master...HEAD -- tests/golden/`: vacío.
- Los catálogos inválidos fallan con transición, índice de guard, tag y
  expectativa en el mensaje.
- Los tests de ciclo mantienen paridad de decisión y los goldens JSONL quedan
  byte-idénticos.

La compuerta estricta de Clippy continúa siendo deuda de línea base por warnings
preexistentes fuera de este cambio; las suites de ejecución y los goldens están
verdes.

## Migraciones

Cada guard simple se migró en un commit independiente:

- `Cardinality` → `ocupacion.cardinalidad == ...`
- `PersonPresent` → `persona.presente == true`
- `PersonAbsent` → `persona.presente == false`
- `FaceDetected` → `cara.confianza >= ...`
- `FaceAbsent` → `cara.presente == false`
- `FaceInDwell` → `cara.en_dwell == true`
- `FaceNotInDwell` → `cara.en_dwell == false`
- `FaceAtEdge` → `cara.en_borde == true`
- `FaceNotAtEdge` → `cara.en_borde == false`
- `FaceWasInside` → `cara.estuvo_dentro == true`
- `FaceWasNotInside` → `cara.estuvo_dentro == false`

## Siguiente etapa

La Etapa D puede agregar el evento observable del snapshot completo, conectarlo
al logger y retirar gradualmente `FsmSceneContext` como struct plano. No se debe
mezclar esa limpieza con cambios de política clínica.
