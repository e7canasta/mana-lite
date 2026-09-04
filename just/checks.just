# Preflight checks. These recipes do not build or run the pipeline.

check-doctor:
    #!/usr/bin/env bash
    set -euo pipefail

    repo_root="$(git rev-parse --show-toplevel)"
    cd "$repo_root"

    target_dir="${CARGO_TARGET_DIR:-$HOME/.cache/mana-lite-target}"
    printf 'repo:        %s\n' "$repo_root"
    printf 'commit:      %s\n' "$(git rev-parse --short HEAD)"
    printf 'target dir:  %s\n' "$target_dir"

    test -d ../inference
    printf 'inference:   ok\n'

    for model in \
        tools/model-tools/artifacts/yolo26-fp16/yolo26s-fp16-320.onnx \
        tools/model-tools/artifacts/yoloface-fp16/yolov12s-face-fp16-320.onnx \
        tools/model-tools/artifacts/yolo26-fp16/yolo26s-pose-fp16-320.onnx \
        tools/model-tools/artifacts/yolo26-fp16/yolo26s-seg-fp16-320.onnx
    do
        test -f "$model"
    done
    printf 'models:      ok\n'

    if ss -ltn | grep -Eq '(:8554|:1984)[[:space:]]'; then
        printf 'go2rtc:      listening\n'
    else
        printf 'go2rtc:      NOT LISTENING on 8554/1984\n' >&2
        exit 1
    fi

check-tests-release:
    #!/usr/bin/env bash
    set -euo pipefail

    repo_root="$(git rev-parse --show-toplevel)"
    cd "$repo_root"
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/mana-lite-target}"
    cargo test --workspace --release
