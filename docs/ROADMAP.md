# Mana Lite Roadmap

## v0.1 — Skeleton (week 1)

**Goal:** Single frame through the entire pipeline end-to-end.

- [ ] `Cargo.toml` with dependencies on `retina`, `ort`, `serde`, `serde_json`, `toml`, `ffmpeg-next`
- [ ] `src/config.rs` — parse `mana.toml` into `AppConfig`
- [ ] `src/ingest.rs` — RTSP client wrapping `retina::Session` + ffmpeg software decode
- [ ] `src/infer.rs` — wrap ONNX Runtime session for a single model (port from inference's `model.rs`, stripped of wasm/batch/webcam)
- [ ] `src/main.rs` — minimal superloop: INGEST → INFER → PUBLISH
- [ ] `config/mana.toml` — template with RTSP URL, model path, keyframes_only
- [ ] `config/models.toml` — catalog with one entry

**Deliverable:** `mana-lite --config mana.toml` prints detection JSON lines for every I-frame.

## v0.2 — Model Catalog & I-Frame Gate (week 2)

**Goal:** Multiple model entries in catalog, proper I-frame gating, stale detection.

- [ ] `models.toml` parsed into `HashMap<String, ModelEntry>`
- [ ] I-frame gate via `VideoFrame::is_random_access_point()` from retina codec
- [ ] `Config` health section: `data_stale_ms`, max panic count
- [ ] `Health::Stale` and `Health::Blind` events emitted when frame gap exceeds thresholds
- [ ] CLI: `--model detect-fast` selects which catalog entry to run

**Deliverable:** multi-camera RTSP streams survive reconnect and keep publishing health events.

## v0.3 — Spatial Zones (week 3)

**Goal:** Named ROIs as clinical zones, detection-to-zone mapping.

- [ ] `zones.toml` — named axis-aligned rectangles
- [ ] `src/zones.rs` — `ZoneEngine` that evaluates detections against zone definitions
- [ ] `ZoneChange` events: zone → occupied/vacated with dwell hysteresis
- [ ] Reuse inference's ROI crop + coordinate back-projection for zone-limited inference

**Deliverable:** `{"type":"zone","zone":"bed","event":"occupied","by_class":"person"}`

## v0.4 — Clinical FSM (week 3-4)

**Goal:** Hierarchical state machines driving clinical alert logic.

- [ ] `fsm.toml` — states, transitions, guards, dwells
- [ ] `src/fsm.rs` — `FsmEngine` with guard evaluation, dwell timers
- [ ] `FsmTransition` events with trigger and dwell
- [ ] State-driven model selection: each state declares which models run
- [ ] `Ton`/`Tof` timer primitives (PLC on-delay / off-delay)

**Deliverable:** `{"type":"fsm","from":"monitoring","to":"alarmed","trigger":"bed_exit_attempt","dwell_ms":3000}`

## v0.5 — Cascaded Inference (week 4-5)

**Goal:** Lazy model execution based on detection context → reduce GPU load.

- [ ] `src/cascade.rs` — `CascadeEngine` with parent→child DAG
- [ ] Per-model timers: `interval_min_ms`, `cooldown_ms`
- [ ] Bbox-scoped inference: run pose/face only on regions with person detections
- [ ] Mode: "primary-only" (cheapest model always), "on-demand" (children on guard trigger), "all" (debug)

**Deliverable:** detect runs every 200ms; pose only when person present; face only when head keypoints visible.

## v0.6 — Liveness & Hardening (week 5-6)

**Goal:** Production-adjacent reliability.

- [ ] `Health::Heartbeat` every N cycles with per-phase timing (WCET visibility)
- [ ] Panic recovery: catch panics in ingest/infer phases, increment error counter, restart loop
- [ ] Model load failure → degraded mode with only models that loaded successfully
- [ ] RTSP reconnect with exponential backoff
- [ ] `--output-dir` for file-based JSONL with hourly rotation

**Deliverable:** runs under systemd, survives camera disconnects and model OOMs.

## v1.0 — Hardened Release (week 6-7)

**Goal:** Supervisor-friendly, CI-tested, documented.

- [ ] systemd unit file: `mana-lite.service`
- [ ] CI: `cargo test --workspace`, `cargo clippy -- -D warnings`, `cargo fmt --check`
- [ ] Integration test: synthetic RTSP → detect → zone → FSM transition
- [ ] `manpage` entry for `mana-lite(1)`
- [ ] Debian package: `mana-lite_1.0.0_amd64.deb`

**Deliverable:** stable binary in the hands of clinical integrators.

---

## Beyond v1.0

| Feature | Rationale |
|---|---|
| VAAPI/NVDec hardware decode | Lower CPU on edge devices |
| Zenoh control plane bridge | Interop with full Mana OS |
| Multi-camera (one process per camera) | Same binary, different config |
| Web dashboard | Real-time zone heatmaps for nurses |
| ONNX model hot-swap | Reload model without restart |
| `mana-lite replay` | Offline replay from recorded `.mp4` + log |
