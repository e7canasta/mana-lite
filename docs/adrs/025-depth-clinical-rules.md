# ADR-025: Depth Clinical Rules and Scene Calibration

**Status:** Accepted
**Date:** 2026-08-08

## Context

Depth rules emit robust region evidence, while zones and the FSM express
identity-independent clinical context. The pipeline needs to combine them for
cases such as bed approach or distance to a bed edge without making depth gate
face/segmentation or claiming metric accuracy without a physical reference.

## Decision

1. `DepthCalibration` uses one measured scene reference to define a scale:
   `scene_value = model_value * reference_scene_m / reference_model_m`.
2. Calibration is optional and validated only when both references are finite
   and positive. Without it, values remain explicitly model-relative.
3. `depth_region` is version 2 and records the calibration references when
   present. The comparison occurs after scaling, so `value` and `threshold_m`
   share the same units.
4. FSM gets a `depth_rule` guard. Runtime builds a `DepthRuleSnapshot` for each
   frame; missing evidence is false, never a trigger. FSM validation checks
   referenced rule names against the depth catalog.
5. The clinical combination remains in FSM configuration. For example,
   `watching -> bed_approaching` requires both `zone_occupied(bed)` and
   `depth_rule(bed-approach)`.

## Consequences

- Clinical transitions can combine zone occupancy and depth evidence without
  coupling depth to model scheduling.
- JSONL retains enough calibration metadata to audit a triggered rule.
- A physical scene measurement is required before interpreting the result as a
  metric distance; provisional thresholds remain safe to identify as such.
- Real-camera validation of bed approach, edge distance, and approach/retreat
  behavior remains pending camera access.
