# Standard Library Crates (std/)
La biblioteca estándar del ecosistema **mana-lite** se compone de tres herramientas fundamentales diseñadas como **utilidades de bajo nivel** y alto rendimiento para el procesamiento de datos. El sistema utiliza **mana-geometry** para transformar salidas visuales brutas en estructuras matemáticas precisas, empleando técnicas de **compresión de máscaras** y aritmética espacial para definir objetos. Por otro lado, **mana-id** garantiza la integridad del software mediante **identificadores con seguridad de tipos**, mientras que **mana-media** gestiona la **ingesta y decodificación de video** de manera eficiente a través de buffers reutilizables. En conjunto, estos módulos actúan como un puente esencial que traduce la información sensorial compleja en un **dominio semántico estructurado** y libre de dependencias circulares


Relevant source files

- [](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-geometry/Cargo.toml)
- [](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-geometry/src/lib.rs)
- [](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-id/src/lib.rs)
- [](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-media/Cargo.toml)
- [](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-media/src/lib.rs)

The `std/` directory contains shared utility crates that form the foundation of the `mana-lite` ecosystem. These crates are designed to be low-level, high-performance, and free of dependencies on higher-level logic crates like `mana-control` or `mana-perception` [std/mana-media/src/lib.rs7-8](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-media/src/lib.rs#L7-L8)

## Overview of Shared Utilities

The standard library is partitioned into three specialized domains:

|Crate|Responsibility|Key Entities|
|---|---|---|
|**`mana-geometry`**|Spatial primitives and mask compression.|`BBox`, `CompactMask`, `Polygon`|
|**`mana-id`**|Type-safe domain identifiers.|`DomStr`, `domain_id!`|
|**`mana-media`**|Video frame transport and decoding.|`DecodedFrame`, `FrameDecoder`, `BufferPool`|

### System Integration Map

The following diagram illustrates how these utility crates bridge the gap between raw data (video/geometry) and the semantic domain (identifiers).

**Crate Interaction and Domain Mapping**

**Sources:** [std/mana-id/src/lib.rs1-15](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-id/src/lib.rs#L1-L15) [std/mana-media/src/lib.rs92-104](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-media/src/lib.rs#L92-L104) [std/mana-geometry/src/lib.rs1-4](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-geometry/src/lib.rs#L1-L4)

---

## 7.1 mana-geometry

`mana-geometry` provides the mathematical and spatial primitives required for computer vision tasks. It handles the transition from raw model outputs (bounding boxes and segmentation masks) to structured geometric data.

- **BBox & IoU**: Core arithmetic for bounding boxes and Intersection over Union calculations used in tracking and NMS [std/mana-geometry/src/lib.rs13-15](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-geometry/src/lib.rs#L13-L15)
- **CompactMask**: A memory-efficient, Run-Length Encoded (RLE) storage for segmentation masks. It uses `Arc` for zero-copy sharing across the pipeline [std/mana-geometry/src/lib.rs14](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-geometry/src/lib.rs#L14-L14)
- **Polygonization**: Logic for converting pixel masks into simplified polygons using Douglas-Peucker and validating areas via the Shoelace formula [std/mana-geometry/src/lib.rs17-18](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-geometry/src/lib.rs#L17-L18)

For implementation details on spatial arithmetic and mask compression, see **[mana-geometry](https://deepwiki.com/kerrvisiona-sudo/endeli/7.1-mana-geometry)**.

**Sources:** [std/mana-geometry/src/lib.rs1-20](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-geometry/src/lib.rs#L1-L20) [std/mana-geometry/Cargo.toml5-16](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-geometry/Cargo.toml#L5-L16)

---

## 7.2 mana-id and mana-media

### mana-id

This crate implements the mechanism for type-safe string identifiers. It prevents logic errors where a `ZoneId` might be accidentally passed to a function expecting a `ModelId`.

- **`DomStr`**: A newtype wrapper around `Arc<str>` that ensures efficient sharing and deterministic ordering for use in `BTreeMap` [std/mana-id/src/lib.rs12-15](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-id/src/lib.rs#L12-L15) [std/mana-id/src/lib.rs52-58](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-id/src/lib.rs#L52-L58)
- **`domain_id!`**: A macro used by consumer crates to generate specific ID types (e.g., `StateId`, `ClassName`) that are distinct at the type level but share the underlying `DomStr` implementation [std/mana-id/src/lib.rs109-116](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-id/src/lib.rs#L109-L116)

### mana-media

`mana-media` defines the contract for video ingestion and frame processing. It abstracts the complexities of different video backends (e.g., FFmpeg vs. raw streams).

- **`DecodedFrame`**: A unified structure containing `RawFrameV1` metadata and the raw pixel buffer [std/mana-media/src/lib.rs92-104](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-media/src/lib.rs#L92-L104)
- **`FrameDecoder`**: A trait that defines how sources (RTSP, Files, etc.) should yield frames asynchronously [std/mana-media/src/lib.rs118-132](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-media/src/lib.rs#L118-L132)
- **`BufferPool`**: A utility for re-using pixel buffers to minimize allocation overhead during high-frequency inference [std/mana-media/src/lib.rs12](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-media/src/lib.rs#L12-L12)

**Frame Decoding Contract**

**Sources:** [std/mana-media/src/lib.rs21-43](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-media/src/lib.rs#L21-L43) [std/mana-media/src/lib.rs118-132](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-media/src/lib.rs#L118-L132)

For details on type-safe identifiers and frame transport protocols, see **[mana-id and mana-media](https://deepwiki.com/kerrvisiona-sudo/endeli/7.2-mana-id-and-mana-media)**.

**Sources:** [std/mana-id/src/lib.rs1-186](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-id/src/lib.rs#L1-L186) [std/mana-media/src/lib.rs1-132](https://github.com/kerrvisiona-sudo/endeli/blob/ad24740d/std/mana-media/src/lib.rs#L1-L132)

Dismiss

Refresh this wiki

This wiki was recently refreshed. Please wait 7 days to refresh again.

### On this page

- [Standard Library Crates (std/)](https://deepwiki.com/kerrvisiona-sudo/endeli/7-standard-library-crates-\(std\)#standard-library-crates-std)
- [Overview of Shared Utilities](https://deepwiki.com/kerrvisiona-sudo/endeli/7-standard-library-crates-\(std\)#overview-of-shared-utilities)
- [System Integration Map](https://deepwiki.com/kerrvisiona-sudo/endeli/7-standard-library-crates-\(std\)#system-integration-map)
- [7.1 mana-geometry](https://deepwiki.com/kerrvisiona-sudo/endeli/7-standard-library-crates-\(std\)#71-mana-geometry)
- [7.2 mana-id and mana-media](https://deepwiki.com/kerrvisiona-sudo/endeli/7-standard-library-crates-\(std\)#72-mana-id-and-mana-media)
- [mana-id](https://deepwiki.com/kerrvisiona-sudo/endeli/7-standard-library-crates-\(std\)#mana-id)
- [mana-media](https://deepwiki.com/kerrvisiona-sudo/endeli/7-standard-library-crates-\(std\)#mana-media)

...Relevant...

Ask Devin about kerrvisiona-sudo/endeli

Fast

Syntax error in textmermaid version 11.7.0

Syntax error in textmermaid version 11.7.0

Syntax error in textmermaid version 11.7.0

Add to ContextPress Q