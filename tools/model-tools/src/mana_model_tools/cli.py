from __future__ import annotations

import argparse
import hashlib
import shutil
import sys
from collections import Counter
from pathlib import Path


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="model-tools",
        description="Prepare ONNX model variants for the mana-lite model catalog.",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    inspect = commands.add_parser("inspect", help="Show ONNX inputs, outputs, and metadata")
    inspect.add_argument("--input", type=Path, required=True)
    inspect.set_defaults(handler=inspect_model)

    export = commands.add_parser("export", help="Export a Ultralytics checkpoint to ONNX")
    export.add_argument("--model", type=Path, required=True, help="Ultralytics .pt checkpoint")
    export.add_argument("--output", type=Path, required=True, help="Destination .onnx path")
    export.add_argument("--imgsz", type=int, required=True)
    export.add_argument("--device", default="cpu", help="Ultralytics device, e.g. cpu or 0")
    export.add_argument("--half", action="store_true", help="Export the graph using FP16")
    export.add_argument("--dynamic", action="store_true", help="Export dynamic image dimensions")
    export.add_argument("--no-simplify", action="store_true")
    export.add_argument("--opset", type=int)
    export.set_defaults(handler=export_model)

    fp16 = commands.add_parser("fp16", help="Convert an existing ONNX graph to FP16")
    fp16.add_argument("--input", type=Path, required=True)
    fp16.add_argument("--output", type=Path, required=True)
    fp16.set_defaults(handler=convert_fp16)

    int8 = commands.add_parser("int8", help="Statically quantize an ONNX graph to INT8")
    int8.add_argument("--input", type=Path, required=True)
    int8.add_argument("--output", type=Path, required=True)
    int8.add_argument("--data", type=Path, required=True, help="Representative image directory")
    int8.add_argument("--imgsz", type=int, required=True)
    int8.add_argument("--limit", type=int, default=500)
    int8.set_defaults(handler=quantize_int8)

    promote = commands.add_parser("promote", help="Copy a validated artifact into models/")
    promote.add_argument("--input", type=Path, required=True)
    promote.add_argument("--name", required=True, help="Filename to use in the model catalog")
    promote.add_argument("--models-dir", type=Path, default=Path("../../models"))
    promote.add_argument("--catalog-key", required=True, help="TOML model key, e.g. detect-fast-fp16")
    promote.add_argument("--task", required=True, choices=("detect", "pose", "segment", "depth"))
    promote.add_argument("--imgsz", type=int, required=True)
    promote.add_argument("--half", action="store_true")
    promote.add_argument("--force", action="store_true")
    promote.set_defaults(handler=promote_model)

    return parser


def require_file(path: Path, suffix: str | None = None) -> Path:
    if not path.is_file():
        raise SystemExit(f"input file does not exist: {path}")
    if suffix and path.suffix.lower() != suffix:
        raise SystemExit(f"expected {suffix} file, got: {path}")
    return path


def prepare_output(path: Path, suffix: str = ".onnx") -> Path:
    if path.suffix.lower() != suffix:
        raise SystemExit(f"output must use the {suffix} extension: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def inspect_model(args: argparse.Namespace) -> int:
    import onnx

    model_path = require_file(args.input, ".onnx")
    model = onnx.load(str(model_path), load_external_data=False)
    print(f"path: {model_path}")
    print(f"file_bytes: {model_path.stat().st_size:,}")
    print(f"opset: {', '.join(str(opset.version) for opset in model.opset_import)}")
    print("initializer_dtypes:")
    for dtype, count in _initializer_precision_counts(model).items():
        print(f"  - {dtype}: {count}")
    print("inputs:")
    for value in model.graph.input:
        print(f"  - {value.name}: {_tensor_shape(value)}")
    print("outputs:")
    for value in model.graph.output:
        print(f"  - {value.name}: {_tensor_shape(value)}")
    return 0


def _tensor_shape(value: object) -> str:
    tensor_type = value.type.tensor_type  # type: ignore[attr-defined]
    dimensions = []
    for dimension in tensor_type.shape.dim:
        if dimension.HasField("dim_value"):
            dimensions.append(str(dimension.dim_value))
        elif dimension.HasField("dim_param"):
            dimensions.append(dimension.dim_param)
        else:
            dimensions.append("?")
    return "[" + ", ".join(dimensions) + "]"


def export_model(args: argparse.Namespace) -> int:
    from ultralytics import YOLO

    checkpoint = require_file(args.model)
    output = prepare_output(args.output)
    export_options = {
        "format": "onnx",
        "imgsz": args.imgsz,
        "device": args.device,
        "dynamic": args.dynamic,
        "simplify": not args.no_simplify,
        "nms": False,
    }
    if args.half:
        export_options["quantize"] = 16
    if args.opset:
        export_options["opset"] = args.opset
    exported = YOLO(str(checkpoint)).export(**export_options)
    exported_path = Path(exported)
    if not exported_path.is_file():
        raise SystemExit(f"Ultralytics did not produce an ONNX file: {exported_path}")
    shutil.copy2(exported_path, output)
    if args.half:
        _verify_fp16_model(output)
    print(f"wrote {output} ({output.stat().st_size:,} bytes)")
    if args.half:
        print("verified: graph contains FLOAT16 initializers")
    return 0


def convert_fp16(args: argparse.Namespace) -> int:
    import onnx
    from onnxconverter_common import float16

    input_path = require_file(args.input, ".onnx")
    output = prepare_output(args.output)
    if input_path.resolve() == output.resolve():
        raise SystemExit("input and output must be different files")
    model = onnx.load(str(input_path), load_external_data=False)
    converted = float16.convert_float_to_float16(model, keep_io_types=True)
    before = _initializer_precision_counts(model)
    onnx.save(converted, str(output))
    after = _verify_fp16_model(output)
    print(f"wrote {output} ({output.stat().st_size:,} bytes)")
    print(f"initializer_dtypes_before: {dict(before)}")
    print(f"initializer_dtypes_after: {dict(after)}")
    print(f"size_ratio: {output.stat().st_size / input_path.stat().st_size:.3f}")
    return 0


def quantize_int8(args: argparse.Namespace) -> int:
    from onnxruntime.quantization import (
        CalibrationMethod,
        QuantFormat,
        QuantType,
        quantize_static,
    )

    input_path = require_file(args.input, ".onnx")
    if not args.data.is_dir():
        raise SystemExit(f"calibration directory does not exist: {args.data}")
    output = prepare_output(args.output)
    reader = ImageCalibrationReader(input_path, args.data, args.imgsz, args.limit)
    quantize_static(
        str(input_path),
        str(output),
        reader,
        quant_format=QuantFormat.QDQ,
        activation_type=QuantType.QUInt8,
        weight_type=QuantType.QInt8,
        calibrate_method=CalibrationMethod.MinMax,
        per_channel=True,
    )
    print(f"wrote {output} ({output.stat().st_size:,} bytes)")
    return 0


class ImageCalibrationReader:
    def __init__(self, model_path: Path, data_dir: Path, imgsz: int, limit: int) -> None:
        import onnx

        self.input_name = onnx.load(str(model_path), load_external_data=False).graph.input[0].name
        self.images = sorted(
            path
            for path in data_dir.rglob("*")
            if path.suffix.lower() in {".jpg", ".jpeg", ".png", ".bmp", ".webp"}
        )[:limit]
        if not self.images:
            raise SystemExit(f"no calibration images found in {data_dir}")
        self.imgsz = imgsz
        self.index = 0

    def get_next(self) -> dict[str, object] | None:
        if self.index >= len(self.images):
            return None
        image_path = self.images[self.index]
        self.index += 1
        return {self.input_name: _load_image(image_path, self.imgsz)}


def _load_image(path: Path, imgsz: int):
    import cv2
    import numpy as np

    image = cv2.imread(str(path), cv2.IMREAD_COLOR)
    if image is None:
        raise SystemExit(f"could not read calibration image: {path}")
    image = cv2.cvtColor(image, cv2.COLOR_BGR2RGB)
    height, width = image.shape[:2]
    scale = min(imgsz / width, imgsz / height)
    resized = cv2.resize(image, (max(1, round(width * scale)), max(1, round(height * scale))))
    canvas = np.full((imgsz, imgsz, 3), 114, dtype=np.uint8)
    top = (imgsz - resized.shape[0]) // 2
    left = (imgsz - resized.shape[1]) // 2
    canvas[top : top + resized.shape[0], left : left + resized.shape[1]] = resized
    return (canvas.transpose(2, 0, 1)[None].astype(np.float32) / 255.0)


def promote_model(args: argparse.Namespace) -> int:
    input_path = require_file(args.input, ".onnx")
    if Path(args.name).name != args.name or not args.name.endswith(".onnx"):
        raise SystemExit("--name must be a simple .onnx filename")
    destination = args.models_dir / args.name
    if destination.exists() and not args.force:
        raise SystemExit(f"destination exists; use --force to replace it: {destination}")
    if not args.models_dir.is_dir():
        raise SystemExit(f"models directory does not exist: {args.models_dir}")
    if args.half:
        _verify_fp16_model(input_path)
    shutil.copy2(input_path, destination)
    print(f"promoted {input_path} -> {destination}")
    print(f"sha256: {_sha256(destination)}")
    print()
    print(f"[models.{args.catalog_key}]")
    print(f'path = "models/{args.name}"')
    print(f'task = "{args.task}"')
    print(f"imgsz = {args.imgsz}")
    if args.half:
        print("half = true")
    return 0


def _initializer_precision_counts(model: object) -> Counter[str]:
    import onnx

    return Counter(onnx.TensorProto.DataType.Name(initializer.data_type) for initializer in model.graph.initializer)  # type: ignore[attr-defined]


def _verify_fp16_model(path: Path) -> Counter[str]:
    import onnx

    model = onnx.load(str(path), load_external_data=False)
    counts = _initializer_precision_counts(model)
    if counts.get("FLOAT16", 0) == 0:
        raise SystemExit(
            f"FP16 verification failed for {path}: no FLOAT16 initializers; "
            f"found {dict(counts)}"
        )
    return counts


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    args = build_parser().parse_args()
    try:
        return args.handler(args)
    except KeyboardInterrupt:
        print("interrupted", file=sys.stderr)
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
