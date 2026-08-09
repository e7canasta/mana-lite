# Overview

Relevant source files

- [](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/.gitignore)
- [](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/README.md?plain=1)
- [](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/lib.rs)
- [](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs)

The `mana-lite` system is a high-performance, real-time room presence and face-detection pipeline. It is designed to ingest RTSP video streams and execute a multi-stage computer vision pipeline including object detection, pose estimation, segmentation, and depth analysis. The system tracks individuals across frames, evaluates spatial occupancy within defined zones, and drives a finite state machine (FSM) to determine complex room states such as occupancy cardinality and specific behavioral events (e.g., "in bed" or "exiting").

[README.md1-6](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/README.md?plain=1#L1-L6)

## System Architecture

The application is structured as a linear pipeline managed by the `App` struct [src/main.rs65-94](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L65-L94) The lifecycle of a video frame involves ingestion, decoding, inference, and sequential logic processing.

### Data Flow Overview

1. **Ingest**: The `IngestEngine` [src/main.rs76](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L76-L76) utilizes `RetinaReader` [src/main.rs36](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L36-L36) to capture H.264 Annex-B streams from RTSP sources.
2. **Inference**: The `InferEngine` [src/main.rs66](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L66-L66) executes models defined in a central `ModelCatalog`. It supports cascading models, where a primary detection (e.g., a person) triggers secondary crops for higher-resolution analysis (e.g., a face).
3. **Tracking & Spatial Logic**: Detections are fed into a `Tracker` [src/main.rs68](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L68-L68) (using Kalman filters and Hungarian assignment). The resulting tracks are evaluated against a `ZoneEngine` [src/main.rs69](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L69-L69) to determine spatial presence.
4. **State Evaluation**: The `OccupancyStateMachine` [src/main.rs80](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L80-L80) and `FsmEngine` [src/main.rs70](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L70-L70) consume presence and depth data to transition between high-level application states.
5. **Observability**: Data is simultaneously published via `VizBridge` [src/main.rs90](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L90-L90) to Rerun.io and logged as structured JSONL by the `LogManager` [src/main.rs37](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L37-L37)

**Sources:** [src/main.rs1-94](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L1-L94) [README.md10-21](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/README.md?plain=1#L10-L21)

### Code Entity Mapping: Pipeline Logic

The following diagram maps the logical stages of the pipeline to the primary Rust structs and modules responsible for their execution.

```mermaid
flowchart TD

    subgraph INGEST["Ingest & Decode"]
        RR["RetinaReader<br/>(src/ingest.rs)"]
        FD["FrameDecoder<br/>(src/snapshot.rs)"]
        RR --> FD
    end

    subgraph IT["Inference & Tracking"]
        IE["InferEngine<br/>(src/infer.rs)"]
        TR["Tracker<br/>(src/track.rs)"]
        IE --> TR
    end

    subgraph SSL["Spatial & State Logic"]
        ZE["ZoneEngine<br/>(src/zones.rs)"]
        OSM["OccupancyStateMachine<br/>(src/occupancy.rs)"]
        FSM["FsmEngine<br/>(src/fsm.rs)"]

        ZE --> OSM
        OSM --> FSM
    end

    subgraph OO["Output & Observability"]
        LM["LogManager<br/>(src/logger.rs)"]
        VB["VizBridge<br/>(src/viz.rs)"]
    end

    FD --> IE
    TR --> ZE
    FSM --> LM
    FSM --> VB
```
**Sources:** [src/main.rs65-94](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L65-L94) [src/main.rs22-51](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L22-L51)

## Workspace Layout

The project is organized as a Cargo workspace to separate core logic from reusable utility crates located in the `std/` directory.

|Component|Path|Description|
|---|---|---|
|**Application**|`src/`|The main binary logic, including the `App` loop and pipeline orchestration.|
|**mana-types**|`std/mana-types`|Shared primitives like `RawFrameV1` and `PixelFormat`.|
|**mana-geometry**|`std/mana-geometry`|Spatial math, `CompactMask` (RLE), and polygon operations.|
|**mana-video**|`std/mana-video`|Video decoding utilities and `BufferPool` management.|
|**mana-rtsp**|`std/mana-rtsp`|Low-level H.264 Annex-B and RTSP stream utilities.|
|**mana-viz**|`std/mana-viz`|The bridge for logging data to the Rerun.io visualization engine.|

For a detailed breakdown of these crates, see [Workspace Crates (#1.2)](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/Workspace%20Crates%20\(#1.2\))

**Sources:** [README.md8-21](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/README.md?plain=1#L8-L21)

## Building and Running

The system is written in Rust (Edition 2024). Configuration is managed through a layered approach using `.toml` files and environment variables.

### Build Requirements

- **Rust Toolchain**: Edition 2024.
- **Dependencies**: The project relies on `tokio` for the async runtime and `ultralytics-inference` for model execution.

```
cargo build --release
```

### Execution Flow

The binary starts by loading the `AppConfig` via `load_app_config` [src/main.rs60](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L60-L60) It then initializes the `App` state through the `bootstrap` function [src/main.rs109](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L109-L109) which validates the model catalog and blueprints before starting the main processing loop.

For detailed setup and environment variable configuration (e.g., `MANA_SOURCE_USERNAME`), see [Getting Started (#1.1)](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/Getting%20Started%20\(#1.1\))

**Sources:** [README.md31-40](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/README.md?plain=1#L31-L40) [src/main.rs55-63](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L55-L63)

### Code Entity Mapping: Configuration & Setup

This diagram illustrates how configuration files and environment variables are ingested into the system's internal state.


```mermaid
flowchart LR

    subgraph CONFIG["Configuration Files"]
        direction TB
        MANA["mana.toml"]
        MODELS["models.toml"]
        BLUEPRINT["blueprint.toml"]
    end

    subgraph SETUP["Setup Logic (src/main.rs)"]
        direction TB
        LOAD_APP["load_app_config"]
        LOAD_MODELS["load_model_catalog"]
        LOAD_CONFIG["load_config&lt;BlueprintCon"]
    end

    subgraph RUNTIME["Runtime State (struct App)"]
        direction TB
        INFER["infer: InferEngine"]
        CASCADE["cascade:<br/>CascadeScheduler"]
    end

    MANA --> LOAD_APP
    MODELS --> LOAD_MODELS
    BLUEPRINT --> LOAD_CONFIG

    LOAD_APP --> INFER
    LOAD_MODELS --> INFER
    LOAD_CONFIG --> CASCADE

    classDef config fill:#fff,stroke:#d9d9d9,color:#333
    classDef setup fill:#fff,stroke:#d9d9d9,color:#333
    classDef runtime fill:#fff,stroke:#d9d9d9,color:#333

    class MANA,MODELS,BLUEPRINT config
    class LOAD_APP,LOAD_MODELS,LOAD_CONFIG setup
    class INFER,CASCADE runtime
```
**Sources:** [src/main.rs59-61](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L59-L61) [src/main.rs126-140](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L126-L140) [src/main.rs65-72](https://github.com/ernestovisiona-netizen/kik8/blob/43a92847/src/main.rs#L65-L72)

## Child Pages

- **[Getting Started](https://deepwiki.com/ernestovisiona-netizen/kik8/1.1-getting-started)**: Environment setup, `mana.toml` configuration, and RTSP stream authentication.
- **[Workspace Crates](https://deepwiki.com/ernestovisiona-netizen/kik8/1.2-workspace-crates)**: Technical details on the supporting library crates in `std/`.


### On this page

- [Overview](https://deepwiki.com/ernestovisiona-netizen/kik8/1-overview#overview)
- [System Architecture](https://deepwiki.com/ernestovisiona-netizen/kik8/1-overview#system-architecture)
- [Data Flow Overview](https://deepwiki.com/ernestovisiona-netizen/kik8/1-overview#data-flow-overview)
- [Code Entity Mapping: Pipeline Logic](https://deepwiki.com/ernestovisiona-netizen/kik8/1-overview#code-entity-mapping-pipeline-logic)
- [Workspace Layout](https://deepwiki.com/ernestovisiona-netizen/kik8/1-overview#workspace-layout)
- [Building and Running](https://deepwiki.com/ernestovisiona-netizen/kik8/1-overview#building-and-running)
- [Build Requirements](https://deepwiki.com/ernestovisiona-netizen/kik8/1-overview#build-requirements)
- [Execution Flow](https://deepwiki.com/ernestovisiona-netizen/kik8/1-overview#execution-flow)
- [Code Entity Mapping: Configuration & Setup](https://deepwiki.com/ernestovisiona-netizen/kik8/1-overview#code-entity-mapping-configuration-setup)
- [Child Pages](https://deepwiki.com/ernestovisiona-netizen/kik8/1-overview#child-pages)

