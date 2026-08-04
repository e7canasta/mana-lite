# ADR-009: Pipeline Design — Ownership, Coupling, Testability

**Status:** Accepted
**Date:** 2026-08-03

## Context

Seven modules feed data through seven sequential phases. Rust's ownership model forces us to decide early: who owns the frame buffer? How do phases communicate? How do we test each engine without a camera or GPU?

This ADR captures the cross-cutting design decisions that affect every module.

## Decision 1: Data Ownership — Arena Pattern (`CycleContext`)

**Decision:** A single `CycleContext` struct owns all intermediate data for one superloop cycle. Each phase borrows from it, fills its slot, and returns. At cycle end, vectors are `clear()`'d — no deallocation, no reallocation.

```rust
struct CycleContext {
    frame: Option<DecodedFrame>,
    detections: Vec<Detection>,
    zone_changes: Vec<ZoneChange>,
    fsm_events: Vec<FsmEvent>,
}
```

**Why not ownership chain:**
```rust
let frame = ingest.poll()?;        // owned
let dets = infer.run(frame)?;      // frame consumed
let zones = zones.eval(&dets)?;    // dets borrowed
```
Ownership chain means each phase consumes the previous phase's output. You cannot retry a phase without re-acquiring the input. The borrow chain approach (`fn run(&self, frame: &Frame)`) requires explicit lifetime annotations on every phase function. The arena avoids both problems: one allocation at cycle start, `clear()` between cycles, zero-copy references within the cycle.

**Why not a god object:** `CycleContext` is data, not behavior. It has no methods beyond `clear()`. Each phase module owns its logic. The context is just the shared scratchpad.

## Decision 2: Phase Coupling — Return Values

**Decision:** Each phase function takes `&mut CycleContext` and returns `Result<PhaseOutcome>`. Phases are pure functions of the context — no hidden state, no side channels.

```rust
#[derive(PartialEq)]
enum PhaseOutcome {
    Ran,      // phase produced data, continue
    Skipped,  // no work needed (no keyframe, no dwell change)
    Degraded, // ran but with warnings
}
```

The superloop is a flat sequence of phase calls:

```rust
loop {
    let _ = phase_ingest(&mut ingest, &mut ctx).await?;
    let outcome = phase_infer(&mut infer, &mut ctx)?;
    if outcome != PhaseOutcome::Skipped {
        phase_zones(&zones, &mut ctx)?;
        phase_fsm(&mut fsm, &mut ctx)?;
    }
    phase_publish(&mut log, &ctx)?;
}
```

The `PhaseOutcome` enum lets the superloop skip downstream phases when upstream produced nothing (e.g., no new i-frame → skip INFER → skip ZONES → skip FSM → still PUBLISH health). This is the ghost mode pattern from ADR-007: dwell timers advance, zone state is re-evaluated with stale detections, health events fire — without running costly phases unnecessarily.

## Decision 3: Testability — Generics with Static Dispatch

**Decision:** Each engine module uses generics constrained by a local trait for its external dependency. Tests use stub implementations. Production uses real implementations. Static dispatch (monomorphization) — no vtable overhead.

```rust
// ingest.rs
pub trait FrameReader {
    async fn next_frame(&mut self) -> Option<VideoFrame>;
}

pub struct IngestEngine<R: FrameReader> {
    reader: R,
    decoder: Decoder,
    // ...
}

// Test:
struct StubReader { frames: VecDeque<VideoFrame> }
impl FrameReader for StubReader { ... }

// Production:
struct RetinaReader { demuxed: retina::Demuxed }
impl FrameReader for RetinaReader { ... }
```

**Why not trait objects (`Box<dyn FrameReader>`):** vtable dispatch per call. For a pipeline running at 0.5-1fps, this is irrelevant performance-wise. But generics cost nothing at runtime and the compilation overhead for 1-2 impls is negligible.

**Why not integration tests with real RTSP + ONNX:** Require network, camera, GPU. CI-hostile. Unit tests with stubs run in <1ms without external services. Integration tests will exist but as a separate CI job with hardware requirements.

The `ModelRunner` trait for inference follows the same pattern:

```rust
pub trait ModelRunner {
    fn run(&mut self, frame: &DecodedFrame) -> Result<Vec<Detection>>;
}

struct OrtRunner { session: ort::Session }
impl ModelRunner for OrtRunner { ... }

struct StubRunner { detections: Vec<Detection> }
impl ModelRunner for StubRunner { ... }
```

## Decision 4: Serialization — Migrate to Serde at v0.2

**Decision:** Keep manual `write_event()` JSON serialization for v0.x. Migrate to `serde` in v0.2 when the event schema stabilizes.

**Rationale:** The manual serializer (340 lines) was written to prove zero-allocation is achievable and to establish exact field abbreviations (`f` not `frame`, `bb` not `bbox`). It works. It's tested. But every new Event variant requires 30+ lines of `buf.extend_from_slice` — this is maintenance overhead, not performance optimization. Serde with `#[serde(rename_all = "camelCase")]` and a custom float serializer (`write_f32` fixed-point logic as a `Serializer` impl) would be 50 lines of derive macros replacing 340 lines of manual formatting. The migration is low-risk: event output format is the contract, not the serialization mechanism. Tests capture exact JSON output, so any regression is caught immediately.

**Not migrating now** because the event schema is still evolving (zone, detection, FSM variants are being added as those modules are built). Manual serialization makes schema changes explicit and grep-able. Once the schema stabilizes at v0.2 (all phase modules implemented), serde migration is a mechanical refactor.

## Decision 5: Feature Flag `video` — Remove

**Decision:** Remove the `video` Cargo feature flag. `ffmpeg-next` and `image` are always compiled.

**Rationale:** 100% of real usage requires video decode. The "no video" mode (synthetic data only) doesn't exist in practice — even replay mode needs ffmpeg to decode `.mp4` files. The feature flag adds `#[cfg(feature = "video")]` annotations that complicate every module touching frames, for a use case that has never been requested. YAGNI.

If a headless simulation mode is needed in the future, it can be a separate binary (`mana-lite-sim`) that depends on `mana-lite` as a library with the data types but not the ingest module. That's a clean separation, not a compile-time flag.

## Decision 6: Error Recovery — Graceful Degradation

**Decision:** Each phase returns `Result`. The superloop catches errors at phase boundaries and degrades rather than panicking.

```rust
let ingest_outcome = match phase_ingest(&mut ingest, &mut ctx).await {
    Ok(outcome) => outcome,
    Err(e) => {
        log.emit(Event::health_stale("ingest", 0));
        ctx.health.ingest = ComponentHealth::Degraded { reason: e.to_string() };
        PhaseOutcome::Skipped
    }
};
```

Phase-level error handling:
- **INGEST disconnect:** mark degraded, continue loop, health timers fire → `BLIND` after `data_stale_ms`
- **INFER panic:** catch (in v0.6), skip model, continue with remaining models
- **Model load failure:** skip model, mark FSM guards referencing it as `false`
- **All models failed:** degraded mode — still emit health events, still respond to SIGTERM

This is NOT full panic recovery (v0.6). For v0.1, errors propagate to main and terminate the process. The infrastructure (`ComponentHealth`, `PhaseOutcome::Degraded`) exists but is not wired.

## Decision 7: Logging — Two Layers

**Decision:** Keep Rust's `log` crate for human-readable operational logging (stderr) and our `Logger` for machine-readable clinical events (stdout JSONL). Never mix them.

```
stderr (log crate):  [INFO] mana-lite v0.1.0 starting
                     [INFO] source: rtsp://192.168.1.100:554/stream1
                     [ERROR] fsm validation: unknown model 'detect-v3'

stdout (JSONL):      {"t":"...","type":"meta","event":"startup","v":"0.1.0"}
                     {"t":"...","type":"detection","f":42,"m":"detect-fast",...}
                     {"t":"...","type":"fsm","from":"watching","to":"alarmed",...}
```

- `log::info!` is for the operator: "what is mana-lite doing right now?"
- `Logger::emit()` is for downstream systems: "what clinical event just happened?"

No clinical event ever appears in stderr. No debug message ever appears in stdout JSONL.

## Consequences

- **Positive:** `CycleContext` arena gives predictable memory behavior — one allocation at startup, `clear()` per cycle, zero alloc in hot path.
- **Positive:** Generic traits make every engine testable without external services.
- **Positive:** `PhaseOutcome` enum enables ghost mode (skip INFER/ZONES/FSM when no new data) without special-case branches.
- **Positive:** Two-layer logging means downstream consumers can `grep -v '^{'` on stderr to see only operational messages, and `mana-lite | clinical-consumer` on stdout for structured events.
- **Negative:** Generics add type parameters to every Engine struct. `IngestEngine<R: FrameReader>` propagates to any struct that holds an `IngestEngine`. Mitigation: only `main.rs` constructs engines. Tests construct with `StubReader`. The generic parameter is invisible to callers.
- **Negative:** Serde migration at v0.2 is deferred work. The manual serializer grows with each new Event variant. This is intentional: the event schema is the contract, and manual serialization makes schema changes explicit.
