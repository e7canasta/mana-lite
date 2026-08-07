from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

from mana_model_tools.cli import build_parser, convert_fp16, prepare_output


def test_parser_exposes_conversion_commands() -> None:
    parser = build_parser()

    assert parser.parse_args(["inspect", "--input", "model.onnx"]).command == "inspect"
    assert parser.parse_args(["fp16", "--input", "a.onnx", "--output", "b.onnx"]).command == "fp16"
    assert parser.parse_args(
        ["int8", "--input", "a.onnx", "--output", "b.onnx", "--data", "calibration", "--imgsz", "320"]
    ).command == "int8"
    assert parser.parse_args(
        [
            "promote",
            "--input",
            "depth.onnx",
            "--name",
            "yolo26s-depth-fp16-320.onnx",
            "--catalog-key",
            "depth-standard",
            "--task",
            "depth",
            "--imgsz",
            "320",
            "--half",
        ]
    ).task == "depth"


def test_prepare_output_creates_parent(tmp_path: Path) -> None:
    output = prepare_output(tmp_path / "nested" / "model.onnx")

    assert output.parent.is_dir()


def test_fp16_conversion_changes_initializer_type(tmp_path: Path) -> None:
    graph = helper.make_graph(
        [helper.make_node("Add", ["input", "weight"], ["output"])],
        "fp32-test",
        [helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 3, 8, 8])],
        [helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 3, 8, 8])],
        initializer=[numpy_helper.from_array(np.ones((1, 3, 8, 8), dtype=np.float32), "weight")],
    )
    source = tmp_path / "source.onnx"
    target = tmp_path / "target.onnx"
    onnx.save(helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)]), source)

    assert convert_fp16(argparse_namespace(source, target)) == 0
    converted = onnx.load(target, load_external_data=False)

    assert converted.graph.initializer[0].data_type == TensorProto.FLOAT16


def argparse_namespace(source: Path, target: Path):
    return type("Args", (), {"input": source, "output": target})()
