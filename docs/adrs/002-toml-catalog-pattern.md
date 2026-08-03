# ADR-002: TOML Catalog Pattern

**Status:** Accepted
**Date:** 2026-08-03

## Context

Mana Lite needs to configure models, zones, and FSM rules. The full Mana OS uses Rust builder-pattern structs and CLI flags. For Mana Lite, we want:

1. **Versionable artifacts:** `models.toml` checked into git → can A/B test `detect-v1` vs `detect-v2` by changing one line in `fsm.toml`.
2. **Separation of concerns:** Models (data science), zones (facility layout), and FSM (clinical policy) are owned by different teams. One file per domain.
3. **Discoverability:** A new integrator reads `models.toml` and sees all available models at a glance.

## Decision

Four TOML files, each with a single responsibility:

| File | Responsibility | Owned by |
|---|---|---|
| `mana.toml` | Application root: source, health, refs to other files | DevOps |
| `models.toml` | ONNX catalog: path, task, thresholds | ML Engineer |
| `zones.toml` | Spatial ROIs: named rectangles | Facility Manager |
| `fsm.toml` | Clinical states, guards, transitions | Clinical Engineer |

The root `mana.toml` references the others by path, not by convention:

```toml
[inference]
model_catalog = "config/models.toml"     # explicit
zones_file = "config/zones.toml"
fsm_file = "config/fsm.toml"
```

This allows multiple configurations to coexist (e.g., `config/prod/models.toml` vs `config/test/models.toml`).

## Why TOML over YAML/JSON

- **TOML:** Standard in Rust ecosystem (cargo uses it). Supports comments. Maps cleanly to serde. No implicit type coercion.
- **YAML:** Overly complex (anchors, tags, multi-document). No-float parsing quirks (Norway problem).
- **JSON:** No comments. Manual editing is painful for non-engineers (zones, FSM).

## Catalog Keys and Versioning

Model keys in `models.toml` are stable identifiers. The FSM and cascade reference them by key, never by path:

```toml
# fsm.toml
[states.watching]
models = ["detect-fast", "pose-standard"]
```

To A/B test a new model, an ML engineer:

1. Adds `[models.detect-v2]` to `models.toml`
2. Changes `models = ["detect-v2", "pose-standard"]` in `fsm.toml`
3. Restart. No recompilation. No code change.

## Consequences

- **Positive:** Git-diff friendly. Model swaps = one-line config change.
- **Positive:** Serde validation at startup catches typos before the superloop runs.
- **Positive:** Zones and FSM can be edited by non-Rust-developers.
- **Negative:** Four files to keep in sync. The root `mana.toml` explicitly points to the others, so there's no hidden convention.
- **Negative:** TOML doesn't support `include` or `import`. Each file is self-contained. Cross-file references (FSM → model keys) are validated at startup with clear error messages.
