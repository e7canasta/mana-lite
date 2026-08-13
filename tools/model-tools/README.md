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
  --output artifacts/yolo26s-fp16-192.onnx
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

Export the full small/medium/large/xlarge FP16 matrix for detection, pose and
segmentation at both 192 and 320:

```bash
./scripts/export-yolo26-fp16-matrix.sh
```

The script expects these detection, pose and segmentation checkpoints in
`../../models`:

```text
yolo26{s,m,l,x}.pt
yolo26{s,m,l,x}-{pose,seg}.pt
```

It writes 24 artifacts under `artifacts/yolo26-fp16/`. Existing artifacts are
skipped so the command can be resumed; use `--force` to regenerate them. Missing
checkpoints are reported at the end and cause a non-zero exit status. Use
`--dry-run` to inspect the matrix first, or restrict it with `--tasks`, `--models`,
and `--sizes`.

Export the YOLO face matrix from the downloaded YOLO11 and YOLO12 checkpoints:

```bash
./scripts/export-yoloface-fp16-matrix.sh
```

The face script accepts `yolov11{s,m,l}-face.pt` and `yolov12{s,m,l}-face.pt`,
including browser-download suffixes such as `yolov12s-face (1).pt`. It writes
12 artifacts under `artifacts/yoloface-fp16/`:

```text
yolov{11,12}{s,m,l}-face-fp16-{192,320}.onnx
```

Use `--dry-run`, `--versions`, `--models`, and `--sizes` to inspect or restrict
the matrix. The corresponding disabled catalog keys are
`face-v{11,12}-{s,m,l}-{192,320}`. They include the same face crop and
postprocess policy as the active `face-yolo` entry and can be enabled one at a
time after latency and detection quality have been compared.

The runtime catalog registers the same matrix with keys such as
`detect-s-320`, `pose-m-192`, and `seg-l-320`. They are disabled
by default in `config/models.toml`; enable one only after selecting it in the
cascade and benchmarking its quality and latency.

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
