# mana-model-tools

Side project for preparing ONNX artifacts used by `mana-lite`. It is deliberately
separate from the Rust runtime: conversion and validation happen here, while the
runtime only consumes approved files from `models/`.

## Setup

From this directory:

```bash
uv sync --extra export
```

The `export` extra installs Ultralytics. The base environment is enough for ONNX
inspection, FP16 conversion, and INT8 calibration.

## Commands

Inspect an existing graph before changing it:

```bash
uv run model-tools inspect --input ../../models/yolov12l-face.onnx
```

Export a checkpoint at a specific input size:

```bash
uv run model-tools export \
  --model /path/to/yolo26s.pt \
  --output artifacts/yolo26s-fp16-320.onnx \
  --imgsz 320 \
  --half
```

Convert an existing ONNX graph to FP16 without needing the original checkpoint:

```bash
uv run model-tools fp16 \
  --input ../../models/yolo26s.onnx \
  --output artifacts/yolo26s-fp16-640.onnx
```

The command verifies that the resulting graph contains `FLOAT16` initializers
and reports the before/after dtype counts and file-size ratio. Keeping model
inputs and outputs as `FLOAT32` is intentional for runtime compatibility; it does
not mean the weights stayed at FP32.

Create a statically calibrated INT8 variant from representative camera images:

```bash
uv run model-tools int8 \
  --input ../../models/yolo26s.onnx \
  --output artifacts/yolo26s-int8-640.onnx \
  --data calibration/ \
  --imgsz 640
```

`int8` expects images that resemble production inputs. Include small objects,
different lighting, occlusion, and empty scenes. Dynamic quantization is not used
because it is generally a poor first choice for convolution-heavy YOLO graphs.

Export the full small/medium/large/xlarge FP16 matrix for detection, pose,
segmentation, and depth at both 320 and 640:

```bash
./scripts/export-yolo26-fp16-matrix.sh
```

The script expects these checkpoints in `../../models`:

```text
yolo26{s,m,l,x}.pt
yolo26{s,m,l,x}-{pose,seg,depth}.pt
```

It writes 32 artifacts under `artifacts/yolo26-fp16/`. Existing artifacts are
skipped so the command can be resumed; use `--force` to regenerate them. Missing
checkpoints are reported at the end and cause a non-zero exit status. Use
`--dry-run` to inspect the matrix first, or restrict it with `--tasks`, `--models`,
and `--sizes`.

## Promotion workflow

Artifacts stay in `artifacts/` until they have been benchmarked and compared with
the current model. Promotion copies one approved file to the model catalog
directory and prints the TOML block to add manually:

```bash
uv run model-tools promote \
  --input artifacts/yolo26s-fp16-320.onnx \
  --name yolo26s-fp16-320.onnx \
  --models-dir ../../models \
  --catalog-key detect-fast-fp16-320 \
  --task detect \
  --imgsz 320 \
  --half
```

The command refuses to overwrite an existing model unless `--force` is supplied.
It also prints a SHA-256 digest for recording in the review or deployment notes.
It does not edit `config/models.toml` or enable a new model automatically.

## Important limitations

- FP16 conversion can be done from ONNX; changing the exported image size is more
  reliable from the original Ultralytics checkpoint.
- `--half` in `export` means FP16 graph export. It is independent from `--imgsz`.
- Validate detection, face, pose, and mask quality separately. Start with FP16
  detection and face variants; treat INT8 segmentation as a later experiment.
- Keep the original ONNX files as baselines until latency and output quality have
  been measured on the target runtime.
