#!/usr/bin/env python3
"""Build a reproducible posture-signature matrix from diagnostic JSON files.

The matrix is descriptive. It combines 2D geometry, segmentation coverage,
keypoint coordinates and scene-depth body-part statistics without pretending
that model depth values are physical meters.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from statistics import fmean


GROUPS = {
    "head": ("nose", "left_eye", "right_eye", "left_ear", "right_ear"),
    "shoulders": ("left_shoulder", "right_shoulder"),
    "hips": ("left_hip", "right_hip"),
    "knees": ("left_knee", "right_knee"),
    "ankles": ("left_ankle", "right_ankle"),
}


def finite_mean(values: list[float]) -> float | None:
    values = [value for value in values if math.isfinite(value)]
    return fmean(values) if values else None


def normalized_point(point: list[float], bbox: list[float]) -> list[float]:
    x1, y1, x2, y2 = bbox
    width = max(x2 - x1, 1e-6)
    height = max(y2 - y1, 1e-6)
    return [(point[0] - x1) / width, (point[1] - y1) / height]


def normalized_bbox(bbox: list[float], reference: list[float]) -> list[float]:
    x1, y1, x2, y2 = reference
    width = max(x2 - x1, 1e-6)
    height = max(y2 - y1, 1e-6)
    return [
        (bbox[0] - x1) / width,
        (bbox[1] - y1) / height,
        (bbox[2] - x1) / width,
        (bbox[3] - y1) / height,
    ]


def group_summary(keypoints: dict[str, dict], names: tuple[str, ...]) -> dict:
    points = [keypoints[name] for name in names if name in keypoints]
    xs = [item["normalized_point"][0] for item in points]
    ys = [item["normalized_point"][1] for item in points]
    depths = [item["depth_m"] for item in points if item.get("depth_m") is not None]
    return {
        "count": len(points),
        "x_mean": finite_mean(xs),
        "y_mean": finite_mean(ys),
        "y_min": min(ys) if ys else None,
        "y_max": max(ys) if ys else None,
        "depth_mean": finite_mean(depths),
        "depth_min": min(depths) if depths else None,
        "depth_max": max(depths) if depths else None,
    }


def load_parts(path: Path) -> dict:
    data = json.loads(path.read_text())
    actors = data.get("actors", [])
    if not actors:
        return {}
    result = {}
    for part in actors[0].get("parts", []):
        depth = part.get("depth") or {}
        result[part["part"]] = {
            "geometry": part.get("geometry"),
            "quality": part.get("quality"),
            "mask_coverage": part.get("mask_coverage"),
            "median_depth": depth.get("median_depth_m"),
            "p10_depth": depth.get("p10_depth_m"),
            "p90_depth": depth.get("p90_depth_m"),
            "relative_to_torso": depth.get("relative_to_torso_m"),
            "surface_evidence": depth.get("surface_evidence", []),
        }
    return {
        "overall_quality": actors[0].get("overall_quality"),
        "parts": result,
    }


def build_signature(radio_path: Path, parts_path: Path) -> dict:
    radio = json.loads(radio_path.read_text())
    person_bbox = radio["person_bbox"]
    width = person_bbox[2] - person_bbox[0]
    height = person_bbox[3] - person_bbox[1]
    keypoints = {}
    for item in radio.get("keypoints", []):
        point = item.get("point")
        if point is None:
            raise ValueError(f"{radio_path}: keypoint coordinates are missing")
        keypoint = dict(item)
        keypoint["normalized_point"] = normalized_point(point, person_bbox)
        keypoints[item["part"]] = keypoint

    shoulders = [keypoints[name]["normalized_point"] for name in GROUPS["shoulders"] if name in keypoints]
    hips = [keypoints[name]["normalized_point"] for name in GROUPS["hips"] if name in keypoints]
    torso_tilt = None
    if len(shoulders) == 2 and len(hips) == 2:
        shoulder = [fmean(point[index] for point in shoulders) for index in (0, 1)]
        hip = [fmean(point[index] for point in hips) for index in (0, 1)]
        torso_tilt = math.degrees(math.atan2(abs(hip[0] - shoulder[0]), abs(hip[1] - shoulder[1])))

    parts = load_parts(parts_path)
    part_values = parts.pop("parts", {})
    body_depths = [
        item["median_depth"]
        for item in part_values.values()
        if item.get("median_depth") is not None
    ]
    coverages = [
        item["mask_coverage"]
        for item in part_values.values()
        if item.get("mask_coverage") is not None
    ]

    face = radio.get("face")
    if face:
        face = dict(face)
        face["normalized_bbox"] = normalized_bbox(face["bbox"], person_bbox)

    return {
        "sample": radio_path.stem,
        "model_key": radio["model_key"],
        "depth_roi": radio["depth_roi"],
        "depth_viz": radio["depth_viz"],
        "person_bbox": person_bbox,
        "person_bbox_width": width,
        "person_bbox_height": height,
        "person_bbox_aspect": width / max(height, 1e-6),
        "segment": radio.get("segment"),
        "face": face,
        "keypoints": keypoints,
        "keypoint_groups": {
            name: group_summary(keypoints, names) for name, names in GROUPS.items()
        },
        "geometry": {
            "torso_tilt_deg": torso_tilt,
        },
        "body_parts": part_values,
        "body_depth_min": min(body_depths) if body_depths else None,
        "body_depth_max": max(body_depths) if body_depths else None,
        "body_depth_spread": (max(body_depths) - min(body_depths)) if body_depths else None,
        "minimum_mask_coverage": min(coverages) if coverages else None,
        "body_parts_quality": parts.get("overall_quality"),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", default="l-640")
    parser.add_argument("--radio-dir", type=Path, default=Path("demo-deep-calib-radio"))
    parser.add_argument("--parts-dir", type=Path, default=Path("demo-deep-calib-parts/results"))
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    radio_dir = args.radio_dir / args.model
    rows = []
    for radio_path in sorted(radio_dir.glob("*.json")):
        parts_path = args.parts_dir / f"{radio_path.stem}.{args.model}.json"
        if not parts_path.is_file():
            raise SystemExit(f"missing body-parts report: {parts_path}")
        rows.append(build_signature(radio_path, parts_path))
    if not rows:
        raise SystemExit(f"no radio reports found in {radio_dir}")
    report = {"model": args.model, "samples": rows}
    encoded = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.write_text(encoded)
    else:
        print(encoded, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
