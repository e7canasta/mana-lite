# ADR-008: Ingest Engine — Async Drain, Keyframe-Only Decode

**Status:** Accepted
**Date:** 2026-08-03

## Context

The superloop needs to read frames from an RTSP camera and deliver the freshest decodable i-frame to the INFER phase. Two forces pull in opposite directions:

1. **The camera is push:** frames arrive at 6-20fps regardless of whether we're ready. Between i-frames, 5-59 P-frames arrive that are useless for inference.
2. **Mana Lite is pull:** the superloop asks for a frame once per cycle. INFER takes 50-300ms. During that window, the camera may deliver an i-frame that we want — not the stale P-frame that was sitting in the buffer.

The design must answer three nested questions:

- **How do we drain stale frames without blocking?** (retina uses `futures::Stream`, not `try_read`)
- **How do we skip P-frames without paying decode cost?** (NAL headers vs ffmpeg)
- **How many frames deep should the buffer be?** (ring of 1? queue of N?)

## Clinical frame cadence (real-world numbers)

| Camera profile | FPS | GOP | i-frames/sec | i-frame interval | P-frames between i-frames |
|---|---|---|---|---|---|
| Standard H.264 | 20 | 40 | 0.5 | 2.0s | 39 |
| Low-latency | 6 | 6 | 1.0 | 1.0s | 5 |
| MJPEG (all I) | 10 | 1 | 10.0 | 100ms | 0 |

The common case: **1 i-frame every 1-2 seconds.** The fastest inference (YOLOv8n, CPU, 320px) is ~50ms. The slowest (YOLOv8x + pose, CPU, 640px) is ~300ms. In the worst realistic case (1 i-frame/s, 300ms inference), the inference phase overlaps at most 1 incoming i-frame. A buffer of 1 slot is sufficient.

## Decision

### 1. Single-slot ring buffer (overwrite semantics)

```rust
struct IngestEngine {
    demuxed: Demuxed,
    last_keyframe: Option<VideoFrame>,  // single slot
    decoder: FfmpegDecoder,
}
```

Each cycle, the engine drains *all* pending frames from the Demuxed stream via non-blocking poll (see decision 2). P-frames are dropped immediately after inspecting `is_random_access_point`. If multiple i-frames arrived, only the most recent survives — the slot is overwritten.

This intentionally discards data. Clinical perception operates at seconds-scale time constants. Processing the most recent complete frame is always correct; processing a stale i-frame from 2 seconds ago would produce stale zone evaluations.

### 2. Non-blocking drain via `tokio::time::timeout(Duration::ZERO, ...)`

Retina exposes a `futures::Stream`, not a `try_read` method. To achieve non-blocking semantics:

```rust
loop {
    match tokio::time::timeout(Duration::ZERO, self.demuxed.next()).await {
        Ok(Some(Ok(CodecItem::VideoFrame(vf)))) => {
            if vf.is_random_access_point() {
                self.last_keyframe = Some(vf);  // overwrite, always freshest
            }
            // P-frame: drop, loop continues
        }
        Ok(None) | Err(_) => break,  // stream ended or buffer empty
        _ => {}  // audio or RTCP: ignore
    }
}
```

`timeout(Duration::ZERO)` polls the stream once. If the socket buffer is empty, it returns `Elapsed` immediately — no blocking, no spin-wait. If data is available, it reads one frame and loops again. The entire drain costs microseconds per frame (RTP depacketization only, no decode).

### 3. Decode only the freshest keyframe, only if changed

After the drain loop, the engine compares the new `last_keyframe` timestamp with the previous cycle's timestamp. If unchanged, it returns `None` — the INFER phase is skipped entirely, and the superloop proceeds to TIMERS → ZONES → FSM → PUBLISH with stale detections (ghost mode, ADR-007).

If the keyframe changed, the engine decodes it via ffmpeg and returns `Some(DecodedFrame)`. The decode is the only expensive operation (~5-20ms) and only happens once per i-frame interval.

### 4. Superloop goes async (`#[tokio::main(flavor = "current_thread")]`)

The INGEST phase requires `async` for the non-blocking drain. This does not violate ADR-003 (PLC superloop) — the superloop remains single-thread and sequential:

```rust
#[tokio::main(flavor = "current_thread")]
async fn main() {
    loop {
        phase_timers();            // sync
        phase_evaluate();          // sync
        phase_ingest().await;      // async: drain + decode
        phase_infer();             // sync (ORT blocks, ok)
        phase_zones();            // sync
        phase_fsm();              // sync
        phase_publish();          // sync
    }
}
```

The async boundary is an implementation detail of the network I/O layer. Every phase completes fully before the next begins. The `current_thread` flavor ensures no work-stealing or task migration — tokio runs on the same OS thread as the superloop.

### 5. Reconnection: drop and recreate

Retina sessions are one-shot: `describe → setup → play → stream`. After disconnect, the session is consumed. Reconnection logic:

```rust
fn reconnect(&mut self) -> Result<()> {
    drop(self.demuxed);  // triggers TEARDOWN
    let session = block_on(Session::describe(url, options))?
        .setup(0, setup_options)?
        .play(play_options)?;
    self.demuxed = session.demuxed()?;
    self.last_keyframe = None;
    Ok(())
}
```

Backoff: 1s, 2s, 4s, 8s, cap 30s. `tokio::time::sleep` between attempts. Reconnection is a phase-level concern: if reconnection is in progress, the INGEST phase skips (returns `None`), and the superloop continues evaluating health timers.

## Alternatives considered

### A. Dedicated ingest thread with blocking read

A background thread reads frames via `Runtime::block_on`, pushes into a `Mutex<Option<VideoFrame>>`. The superloop takes from the Mutex.

**Rejected for v0.x:** Adds thread management and a Mutex for no benefit. The non-blocking drain is simpler and avoids synchronization. May revisit if `timeout(Duration::ZERO)` proves unreliable on certain kernel/network stacks.

### B. `futures::executor::block_on` in a sync superloop

Keep main as `fn main()`, call `block_on(ingest.poll())` inside the sync loop.

**Rejected:** `block_on` inside a loop starves the tokio reactor. Each `block_on` parks and unparks the runtime. Going `#[tokio::main]` with `current_thread` is the idiomatic approach.

### C. Multi-frame buffer (queue of N keyframes)

Instead of a single slot, keep a `VecDeque<VideoFrame>` and process all queued keyframes.

**Rejected:** Clinical perception only cares about the current state. Processing N queued i-frames would mean running inference N times for frames that are 2, 4, 6 seconds old — producing stale events that downstream consumers would ignore.

## Consequences

- **Positive:** I-frame gating at the NAL level saves decode CPU. With GOP=40 at 20fps: 39 P-frames dropped in microseconds each, 1 i-frame decoded. 40:1 reduction in decode cost.
- **Positive:** Single-slot overwrite guarantees the freshest data reaches inference. No queue of stale frames.
- **Positive:** `timeout(Duration::ZERO)` drain is lock-free, allocation-free, and runs in the same thread as the superloop.
- **Negative:** If the tokio `current_thread` runtime is blocked by ORT inference (which is synchronous), the async drain in the *next* cycle may not see frames that arrived during inference until ORT releases the thread. This is acceptable because: (a) the drain empties everything that arrived, regardless of timing, and (b) ORT blocking the thread is exactly the synchronous phase execution we want.
- **Negative:** `Duration::ZERO` timeout semantics are platform-dependent. On Linux, it polls the socket once. On other platforms, it may behave differently. Mitigation: CI tests on Linux x86-64 only. Edge deployment target is Linux ARM64 (Jetson).
- **Negative:** No reconnection in v0.1 skeleton. The engine panics on disconnect. Reconnection is tracked for v0.6 (Liveness & Hardening).

## References

- ADR-003: PLC Superloop Execution Model
- ADR-004: Retina for RTSP Ingest
- ADR-007: I-Frame Gating
- Retina source: `src/client/mod.rs` — `Session::demuxed()`, `Demuxed` as `futures::Stream`
- Retina source: `src/codec/mod.rs` — `VideoFrame::is_random_access_point()`
