# ADR-018: Runtime Stage Boundaries and Publication Contracts

**Status:** Accepted
**Date:** 2026-08-06

## Context

The pipeline has two different kinds of output that were previously described
using the overloaded word `entity`:

- A same-frame observation produced by combining model outputs.
- A temporal identity maintained across frames.

The first must remain usable when tracking is disabled. The second requires
state, lifecycle rules and a `track_id`.

## Decision

Keep the stages independent and make their contracts explicit:

```text
Detection
  -> DetectionConsolidator
  -> ConsolidatedObservation       // stateless, current frame
  -> Tracker (optional)
  -> TrackedEntity                  // temporal identity
```

`DetectionConsolidator` never creates or consumes identity. It reads borrowed
model outputs, fuses same-frame evidence and returns observations. The tracker
is the only component allowed to assign `track_id`, retain misses or predict a
position.

## Publication Contracts

| Output | Requires tracking | Meaning |
|---|---:|---|
| `detection` | No | Raw accepted output per model, diagnostics |
| `consolidated_detection` | No | Spatially fused observation for this frame |
| `entity` | Yes | Temporal identity with `track_id` |
| `/world/camera/observations` | No | Rerun view of consolidated observations |
| `/world/camera/entities` | Yes | Rerun view of tracked entities |

With `pipeline.track = false`, the runtime stops after consolidation and
publishes the first two channels. This is the current calibration mode. With
`pipeline.track = true`, it publishes both observations and tracked entities;
the observation is not replaced or hidden by the entity layer.

## Consequences

- Consolidation can be calibrated independently with real video.
- Tracking bugs cannot contaminate same-frame model association.
- JSONL and Rerun have stable, non-overlapping semantics.
- Tracking can later be improved from greedy IoU to Kalman/Hungarian without
  changing the consolidation contract.
- Secondary evidence TTL remains a tracking-stage concern and is not implied
  by the stateless observation output.
