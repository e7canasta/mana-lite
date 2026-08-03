# ADR-007: I-Frame Gating

**Status:** Accepted
**Date:** 2026-08-03

## Context

Clinical perception pipelines care about *what is happening* over time, not every intermediate frame. Running inference on every decoded frame (30fps) burns GPU cycles on nearly identical inputs. Running inference only on I-frames (every 1-5 seconds at typical GOP=30-150) gives the same clinical information at 1/30th the cost.

The inference codebase already implements this via `--keyframes-only`, but it does so at the FFmpeg decode level: it decodes every frame then checks `decoded.is_key()`. We can do better.

## Decision

**Gate at the NAL level, before decode.**

Retina depacketizes H.264 access units and exposes `VideoFrame::is_random_access_point()`. This inspects NAL unit types without decoding the pixel data. We gate on this:

```rust
// In INGEST phase
let item = ingest.demuxed.next()?;
if let CodecItem::VideoFrame(vf) = item {
    if config.keyframes_only && !vf.is_random_access_point() {
        return None; // skip decode entirely
    }
    let rgb = decode_h264(&vf.data)?;
    // ...
}
```

### Performance

For a 1920×1080 H.264 stream at 30fps with GOP=60 (I-frame every 2 seconds):

| Mode | Decodes/sec | GPU ms/sec |
|---|---|---|
| All frames | 30 | 600ms (30×20ms infer) |
| Keyframes only (FFmpeg-level) | 30 decodes, 0.5 inferences | 12ms decode + 10ms infer |
| Keyframes only (NAL-level) | 0.5 decodes | 6ms decode + 10ms infer |

NAL-level gating saves the decode CPU cost for 29 frames out of 30. For edge devices with software decode, this is significant.

### Ghost Mode

When I-frame gating is active, zone and FSM state are evaluated on every cycle (TIMERS/EVALUATE/ZONES/FSM phases) regardless of whether inference ran. This keeps dwell timers advancing and health events firing. The last known detections are "ghost" — they persist as truth until the next I-frame delivers fresh data.

This is the clinical equivalent of "retained state" in full Mana OS: between I-frames, we trust that the last detection is still valid. The `data_stale_ms` health check provides a safety net: if no I-frame arrives within 10s, we declare `BLIND`.

## Why Not Run Inference on Every Frame

- **20ms inference × 30fps = 600ms GPU time per second → 60% GPU utilization on a single stream.** For 3 cameras on one Jetson: 180% → thermal throttling.
- **Clinical time constants are seconds, not milliseconds.** A bed exit takes 2-5 seconds. 30fps provides no additional clinical signal vs 0.5fps.
- **Consecutive frames differ only by motion vectors.** Detections oscillate ±2px between P-frames. This creates noise in zone occupancy calculation without adding information.

## Why Not Use `--step N` Instead

Stepping every Nth frame is fragile:
- N changes per camera (GOP varies)
- After reconnect, first frame may be a P-frame → decoder error
- No guarantee that step N aligns with keyframe boundaries

I-frame gating always starts decoding from a valid random access point.

## Consequences

- **Positive:** 30-60× reduction in GPU utilization for typical GOP-60 cameras.
- **Positive:** NAL-level gating saves decode CPU, not just inference GPU.
- **Positive:** Ghost mode maintains consistent FSM evaluation cadence regardless of keyframe interval.
- **Negative:** Maximum reaction latency = I-frame interval (1-5s). For bed exit detection, this is clinically acceptable. For fall detection requiring sub-500ms, this is insufficient. Full Mana OS would be required for that use case.
- **Negative:** If the camera's I-frame interval drifts (some cameras adjust GOP dynamically), detection cadence changes. We emit the frame number in every detection event so downstream consumers can monitor cadence.
