# ADR-001: Single Binary Architecture

**Status:** Accepted
**Date:** 2026-08-03

## Context

Full Mana OS is a multi-process DAG communicating via iceoryx2 shared memory and Zenoh. While this provides strong fault isolation and independent scaling, it introduces deployment complexity: 8+ binaries to configure, launch, and supervise. For a "lite" evaluation alternative targeting edge devices and integrators, we need something simpler.

## Decision

Mana Lite is a **single statically-linked binary** with one process, one thread (for the superloop), and zero external IPC dependencies.

Inference runs synchronously in the main thread. Frame decoding runs in a **tokio current-thread runtime** blocked on `block_on(next_frame)`. This is not an async pipeline — it's synchronous blocking with a single future polled per cycle.

## Alternatives Considered

### A. Threaded producer-consumer (like inference's current model)

Producer thread decodes frames, consumer thread runs inference, `std::sync::mpsc::channel` between them.

**Rejected because:** Adds complexity (channel capacity tuning, back-pressure), loses deterministic timing. For a single-camera pipeline, the decode time is a fraction of inference time, so pipelining gains < 5% throughput.

### B. Full DAG with iceoryx2 (like Mana OS)

**Rejected because:** The entire point of Mana Lite is to avoid this complexity for edge/single-camera deployments.

### C. Async tokio with concurrent phases

**Rejected because:** Async race conditions make clinical safety reasoning harder to verify. The PLC superloop pattern is well-understood in safety-critical systems.

## Consequences

- **Positive:** Single binary to deploy. Systemd unit = 10 lines. No IPC configuration. Deterministic execution.
- **Positive:** Easier to debug. `strace`, `perf`, `gdb` all work trivially on one process.
- **Negative:** No fault isolation. A segfault in inference kills the entire pipeline. Mitigated by panic catching at phase boundaries and the `max_consecutive_panics` health check.
- **Negative:** Single-threaded inference means frame throughput is limited to 1/(decode_time + infer_time). Acceptable for a single-camera lite deployment. Full Mana OS handles multi-camera via process-per-camera.
