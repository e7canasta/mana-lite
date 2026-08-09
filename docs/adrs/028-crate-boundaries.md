# ADR-028: Crate Boundaries as Compile-Time Enforcement

**Status:** Accepted
**Date:** 2026-08-09

## Context

El workspace tiene siete crates (`std/mana-{types,video,viz,rtsp,geometry}` más
`core/mana-{control,perception}`). Varios se crearon por afinidad temática, no
porque previnieran algo. El resultado es fronteras que cuestan mantenimiento y
no compran garantías, mientras las fronteras que sí importan (T1↔T2 de ADR-027)
están violadas.

Auditoría de uso real al 2026-08-09:

- `mana-types`: `DetectionV1`, `SceneEntityV1`, `ZoneV1` tienen **cero** usos.
  `DetectionBatchV1`, `SceneMsgV1` y `RoiCommandV1` solo son usados por
  funciones de `mana-viz` que **nadie llama**. Son contrato IPC de Full Mana OS
  (iceoryx2), aspiracional en Mana Lite.
- `mana-viz`: 8 funciones públicas, 2 usadas (`boxes2d_from_xyxy`, `FrameSize`).
- `mana-rtsp`: 64 líneas de helper H.264, un consumidor.

## Decision

### Criterio

En Rust, `pub(crate)` ya provee privacidad de módulo. Un crate separado compra
**una sola cosa** que los módulos no dan: un `Cargo.toml` que hace que una
dependencia prohibida **no compile**.

> Un crate se justifica si y solo si existe una dependencia que queremos volver
> imposible. Si la respuesta es "ninguna", no es un crate: es un módulo.

### Estructura resultante: 5 libs + 1 bin

```
mana-id/          T0  DomStr + macro domain_id!
mana-geometry/    T0  bbox, iou, polygon, compact_mask, transform

mana-media/       T1  PixelFormat, RawFrame, decoder, buffer_pool, h264
mana-perception/  T1  detection, cascade, depth_map, backend ONNX
mana-control/     T2  track, fsm, presence, occupancy, zones, health, kalman

mana-lite/ (bin)  T3  app, config, logger, metrics, viz, ingest, adaptadores
```

### Matriz de dependencias

| | mana-id | geometry | media | perception | control |
|---|:-:|:-:|:-:|:-:|:-:|
| **mana-geometry** | — | — | — | — | — |
| **mana-media** | — | — | — | — | — |
| **mana-perception** | ✔ | ✔ | ✔ | — | **⛔** |
| **mana-control** | ✔ | ✔ | **⛔** | **⛔** | — |
| **mana-lite** | ✔ | ✔ | ✔ | ✔ | ✔ |

Las tres celdas ⛔ son el contenido normativo de este ADR. `control ⛔ media`
expresa que el lazo de control nunca toca un frame, solo observaciones ya
adaptadas.

**Esta matriz se escribe en los `Cargo.toml`, no en documentación.** No se
añade un lint ni un script de CI: el linker es el que la aplica.

### Crates que se disuelven

| Crate | Destino | Razón |
|---|---|---|
| `mana-types` | `PixelFormat` + `RawFrameV1` → `mana-media`; resto borrado | No previene ninguna dependencia; sostiene tipos wire muertos |
| `mana-viz` | → `src/viz/` | Ya es Rerun-específico con un consumidor; no previene nada |
| `mana-rtsp` | → `mana-media` | 64 líneas; no previene nada |

Los tipos `*V1` de iceoryx2 son contrato de **Full Mana OS**, no de Mana Lite.
Su lugar es el repositorio de Full Mana OS. Aquí se borran; git los recuerda.
Esto revisa parcialmente ADR-019.

## Consequences

- **Positivo:** De 7 crates a 5 sin pérdida de capacidad; tres fronteras falsas
  menos que mantener.
- **Positivo:** `mana-control` no puede compilar contra ONNX, FFmpeg, Rerun ni
  percepción. La garantía es mecánica.
- **Positivo:** El `Cargo.toml` de cada crate es legible como declaración de
  arquitectura por un revisor externo en 10 segundos.
- **Negativo:** Borrar los tipos `*V1` cierra la puerta a reusarlos directamente
  si Mana Lite algún día publica por iceoryx2. Se acepta: hoy son código muerto,
  y el contrato real de salida es el JSONL.
- **Negativo:** Disolver `mana-viz` mete código Rerun en el binario. Aceptable:
  el feature `rerun` ya lo hace opcional.

## References

- ADR-027 (tiers), ADR-019 (import mana-os std, parcialmente revisado)
