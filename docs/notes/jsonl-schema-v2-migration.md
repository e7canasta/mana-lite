# JSONL schema v1 → v2 (Fase 2)

Control-loop events (`presence`, `zone`, `entity`, `face_dwell`, and track
`meta` attrs) no longer stamp perception coordinates.

| Removed on control events | Added |
|---|---|
| `frame_id` | `evidence_frame_id` |
| `keyframe_gap_ms` | `scan_seq` |
| `source_window_ms` | `observations_age_ms` |
| `keyframes_seen` / `keyframes_dropped` | `depth_age_ms` (number or `null`) |

Perception events (`frame`, `detection`, `depth*`, `consolidated_detection`)
keep `frame_id` and do not carry `scan_seq`.

`schema` in the startup meta event bumps from `1` to `2`.
