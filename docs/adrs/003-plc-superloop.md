# ADR-003: PLC Superloop Execution Model

**Status:** Accepted
**Date:** 2026-08-03

## Context

The full Mana OS brain engine (`BrainService`) operates on a deterministic superloop inspired by Programmable Logic Controllers (PLCs). This guarantees predictable WCET and eliminates async race conditions in clinical logic. Mana Lite must adopt the same pattern for the same reasons.

## Decision

Mana Lite's main loop is a **synchronous, single-threaded superloop** with seven ordered phases:

```
loop {
    phase_timers();      // advance dwell/ton/tof counters
    phase_evaluate();    // check FSM guards against current state
    phase_ingest();      // read one frame (non-blocking try-read)
    phase_infer();       // run scheduled models
    phase_zones();       // map detections → spatial zones
    phase_fsm();         // apply satisfied transitions
    phase_publish();     // flush event buffer to stdout
}
```

Each phase completes fully before the next begins. No phase spawns background work or yields. The entire cycle is synchronous.

## Why Not Async

- **Race conditions:** Two concurrent FSM evaluations on overlapping data can produce contradictory state transitions. A synchronous loop guarantees each evaluation sees a consistent snapshot.
- **Predictable WCET:** Each phase's execution time is bounded and measurable. Async tasks can interleave unpredictably.
- **Debugging:** A stack trace from a panic in a synchronous loop pinpoints the exact phase and data. Async stack traces are garbage.
- **Simplicity:** The entire runtime is one `loop {}` block. A new developer reads it top-to-bottom in 30 seconds.

## Phase Timing Philosophy

| Phase | Frequency | Rationale |
|---|---|---|
| TIMERS | Every cycle | Sub-µs, no reason to skip |
| EVALUATE | When zone state or timers changed | Could be skipped when idle, but cost is negligible |
| INGEST | Every cycle | Non-blocking try-read; returns immediately if no frame |
| INFER | Per-model timer | Most expensive; gated by `interval_min_ms` per model |
| ZONES | When new detections exist | Sub-µs, guarded by `infer_ran_this_cycle` flag |
| FSM | When zone state or dwells changed | Sub-µs |
| PUBLISH | Every cycle | Flush is O(1) if no events pending |

The loop runs at the natural cadence of the camera (for keyframe-only: every GOP interval, typically 1-5 seconds). Between keyframes, decode is skipped and only timers/evaluate/publish run.

## Health Monitoring Integration

The superloop itself is the health monitor:

- `Health::Heartbeat` emitted every N cycles with per-phase WCET
- `Health::Stale` emitted if INGEST hasn't produced a frame in `data_stale_ms / 2`
- `Health::Blind` emitted if INGEST hasn't produced a frame in `data_stale_ms`
- Panic caught at phase boundaries, error counter incremented, loop continues

## Consequences

- **Positive:** Deterministic. Same inputs → same outputs, always.
- **Positive:** Health-checks are lagging indicators of loop health: if heartbeats stop, the process is truly dead.
- **Positive:** Easy to profile with `perf record` — the superloop is one big function.
- **Negative:** No pipeline parallelism. Inference blocks everything else. Acceptable for single-camera, multi-model lite deployment.
- **Negative:** No graceful shutdown mid-inference. SIGTERM is checked between cycles. If inference takes 500ms, shutdown is delayed by up to 500ms. Acceptable for clinical use (shutdown is rare, inference delay is tolerable).
