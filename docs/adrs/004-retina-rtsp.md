# ADR-004: Retina for RTSP Ingest

**Status:** Accepted
**Date:** 2026-08-03

## Context

Both retina and inference's own `source.rs` can pull frames from RTSP cameras. We need to choose one as the ingress layer for Mana Lite.

**inference `source.rs`** uses `video-rs` (FFmpeg bindings) for RTSP. It works but:
- No session keepalive (RTSP sessions time out on some cameras)
- No RTP sequence tracking (dropped packets go undetected)
- No timestamp enforcement (clock drift between camera and host)
- One-size-fits-all: same code path for files, webcams, RTSP

**retina** is a purpose-built RTSP library with:
- Full RTSP 1.0 implementation (DESCRIBE → SETUP → PLAY → TEARDOWN)
- RTP sequence/SSRC validation (detects packet loss)
- NTP timestamp → wall clock conversion
- Session keepalive (OPTIONS/SET_PARAMETER heartbeats)
- Typed state machine preventing misuse (can't call `play()` before `setup()`)

## Decision

**Use retina for RTSP protocol handling**, then FFmpeg for H.264 → RGB decode.

```
retina::Session → H.264 access units → ffmpeg decode → RGB buffer → inference
```

Retina handles the network (TCP/UDP, RTP, keepalive). FFmpeg handles the pixel decoding (hardware or software). Each does what it's best at.

## I-Frame Detection Without Decoding

Retina's `VideoFrame::is_random_access_point()` inspects NAL unit types in the depacketized H.264 stream. We can detect keyframes before paying the decode cost:

```rust
let item = demuxed.next().await?;
if let CodecItem::VideoFrame(vf) = &item {
    if !vf.is_random_access_point() && config.keyframes_only {
        continue; // skip decode entirely
    }
}
let rgb = decode_h264(&item.data)?;
```

This avoids decoding B/P frames that would be discarded anyway. For a typical GOP=30 camera at 30fps, this saves ~29 decodes per second.

## Reconnection Strategy

Retina sessions can fail (camera reboots, network flaps). Mana Lite wraps reconnect logic:

```rust
fn try_read_frame(ingest: &mut Ingest) -> Option<DecodedFrame> {
    match ingest.session.try_next() {
        Ok(Some(frame)) => Some(DecodedFrame { ... }),
        Ok(None) => None,  // stream ended gracefully
        Err(e) => {
            log.emit(Event::health_stale("ingest", ...));
            ingest.reconnect_with_backoff();
            None
        }
    }
}
```

Backoff: 1s, 2s, 4s, 8s, max 30s. After max backoff, stays at 30s indefinitely.

## Consequences

- **Positive:** Retina's typed session state machine prevents protocol errors at compile time.
- **Positive:** I-frame detection in the NAL layer avoids 97% of unnecessary decodes.
- **Positive:** RTP loss tracking feeds into health monitoring (packet loss → degraded quality → alert).
- **Negative:** Dependency on two low-level libraries (retina + ffmpeg) instead of one (video-rs). Both are well-maintained.
- **Negative:** FFmpeg must be installed on the host. Retina is pure Rust. We document the FFmpeg requirement clearly.
