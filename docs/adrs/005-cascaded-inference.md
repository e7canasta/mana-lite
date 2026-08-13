# ADR-005: Cascaded Inference

**Status:** Historical strategy, superseded by [ADR-026](026-inference-blueprints.md) for deployment selection
**Date:** 2026-08-03

## Context

This ADR records the earlier FSM-driven and timer-gated designs. The current
runtime selects a named blueprint first; its rules then feed the cascade
scheduler. Basic cooperative interval scheduling is implemented; urgent
requests and dynamic same-frame scheduling remain future work.

A clinical deployment might have 5+ models (detect, pose, face, segment, depth). Running all of them on every frame is wasteful. We need a lazy execution model that runs expensive models only when context (detections, zone occupancy, FSM state) justifies the cost.

## Decision

**FSM-driven cascade for v0.4, timer-gated DAG for v0.5.**

### v0.4: State-driven model selection

Each FSM state declares which models it needs:

```toml
[fsm.states.watching]
models = ["detect-fast", "pose-standard"]  # both run when in this state
```

The superloop's INFER phase iterates the active state's model list and runs each one.

### v0.5: Timer-gated cascade DAG

Each model entry in `models.toml` gets optional cascade fields:

```toml
[models.pose-standard]
path = "models/yolo26n-pose.onnx"
task = "pose"
interval_min_ms = 500        # max frequency
requires = "detect-fast"     # parent model
requires_class = "person"    # only when person detected
scope = "bbox"               # crop to bbox before inference
```

The cascade engine:

1. Always runs `detect-fast` (no `requires` = root model).
2. If `detect-fast` found a person above confidence, and 500ms have elapsed since last `pose-standard` run, runs `pose-standard` cropped to the person bbox.
3. If `pose-standard` found head keypoints, and face model interval has elapsed, runs `face-v12`.

```rust
struct CascadeEngine {
    scheduled: Vec<ScheduledModel>,
    last_run: HashMap<String, Instant>,
}

impl CascadeEngine {
    fn models_for_cycle(&self, detections: &[Detection]) -> Vec<&str> {
        let mut models = vec![];
        for entry in &self.scheduled {
            let parent_satisfied = entry.requires.as_ref()
                .map(|req| detections.iter().any(|d| d.class == req))
                .unwrap_or(true);
            let interval_elapsed = self.last_run.get(&entry.key)
                .map(|t| t.elapsed() >= entry.interval)
                .unwrap_or(true);
            if parent_satisfied && interval_elapsed {
                models.push(&entry.key);
            }
        }
        models
    }
}
```

## Why Not Eager (All Models Every Frame)

- **GPU cost:** 5 models × 30fps = 150 inferences per second on a single Jetson. Cascade cuts to ~5-10 inferences per second.
- **Thermal:** Continuous inference overheats edge devices. Cascade allows cooldown between runs.
- **Clinical relevance:** You don't need face ID at 30fps. 0.5fps is sufficient.

## Why Not Full DAG (like Mana OS)

The full DAG model is overkill for a single process. Each "node" in the lite cascade is a synchronous function call, not a separate process. The cascade is a scheduler, not a topology launcher.

## Consequences

- **Positive:** 10-30× reduction in GPU utilization on idle scenes.
- **Positive:** Timer-gating gives predictable maximum load per camera.
- **Positive:** `requires_class` makes pose/face inference context-aware (only where people are).
- **Negative:** Added complexity in the cascade scheduler. Worth it for multi-model deployments, dead weight for single-model.
- **Negative:** `scope = "bbox"` needs the ROI crop machinery from inference. Already exists and is well-tested.
