#!/usr/bin/env bash
set -Eeuo pipefail

# Export the model families that have matching Ultralytics checkpoints:
# s=small, m=medium, l=large, x=xlarge.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd -- "$PROJECT_DIR/../.." && pwd)"

SOURCE_DIR="$REPO_ROOT/models"
OUTPUT_DIR="$PROJECT_DIR/artifacts/yolo26-fp16"
DEVICE="cpu"
DRY_RUN=0
FORCE=0
TASKS=(det pos seg depth)
MODEL_CODES=(s m l x)
INPUT_SIZES=(320 640)

declare -a failures=()
exported=0
skipped=0

usage() {
    cat <<'EOF'
Usage: scripts/export-yolo26-fp16-matrix.sh [options]

Export all requested YOLO26 task/model/input-size combinations to FP16 ONNX.

Options:
  --source-dir DIR   Directory containing yolo26*.pt checkpoints
  --output-dir DIR   Artifact directory (default: artifacts/yolo26-fp16)
  --tasks LIST       Comma-separated: det,pos,seg,depth
  --models LIST      Comma-separated: s,m,l,x
  --sizes LIST       Comma-separated input sizes (default: 320,640)
  --device DEVICE    Ultralytics device (default: cpu)
  --force            Re-export files that already exist
  --dry-run          Print planned exports without running them
  -h, --help         Show this help

Expected checkpoint names:
  yolo26s.pt, yolo26s-pose.pt, yolo26s-seg.pt, yolo26s-depth.pt
  ...and the equivalent m, l, and x variants.
EOF
}

die() {
    printf 'error: %s\n' "$1" >&2
    exit 2
}

split_csv() {
    local value="$1"
    local -n destination="$2"
    IFS=',' read -r -a destination <<< "$value"
    if ((${#destination[@]} == 0)); then
        die "empty list: $value"
    fi
}

contains() {
    local needle="$1"
    shift
    local item
    for item in "$@"; do
        [[ "$item" == "$needle" ]] && return 0
    done
    return 1
}

validate_lists() {
    local task model_code input_size
    for task in "${TASKS[@]}"; do
        contains "$task" det pos seg depth || die "unsupported task: $task"
    done
    for model_code in "${MODEL_CODES[@]}"; do
        contains "$model_code" s m l x || die "unsupported model size: $model_code"
    done
    for input_size in "${INPUT_SIZES[@]}"; do
        [[ "$input_size" =~ ^[0-9]+$ && "$input_size" -gt 0 ]] || die "invalid input size: $input_size"
    done
}

model_names() {
    local task="$1"
    local model_code="$2"
    local source artifact
    case "$task" in
        det)
            source="yolo26${model_code}.pt"
            artifact="yolo26${model_code}-fp16-${CURRENT_INPUT_SIZE}.onnx"
            ;;
        pos)
            source="yolo26${model_code}-pose.pt"
            artifact="yolo26${model_code}-pose-fp16-${CURRENT_INPUT_SIZE}.onnx"
            ;;
        seg)
            source="yolo26${model_code}-seg.pt"
            artifact="yolo26${model_code}-seg-fp16-${CURRENT_INPUT_SIZE}.onnx"
            ;;
        depth)
            source="yolo26${model_code}-depth.pt"
            artifact="yolo26${model_code}-depth-fp16-${CURRENT_INPUT_SIZE}.onnx"
            ;;
    esac
    SOURCE_NAME="$source"
    ARTIFACT_NAME="$artifact"
}

export_one() {
    local task="$1"
    local model_code="$2"
    local input_size="$3"
    CURRENT_INPUT_SIZE="$input_size"
    model_names "$task" "$model_code"

    local source="$SOURCE_DIR/$SOURCE_NAME"
    local output="$OUTPUT_DIR/$ARTIFACT_NAME"
    local command=(
        uv run --project "$PROJECT_DIR" model-tools export
        --model "$source"
        --output "$output"
        --imgsz "$input_size"
        --device "$DEVICE"
        --half
    )

    if [[ -s "$output" && "$FORCE" -eq 0 ]]; then
        printf 'SKIP  %s (already exists)\n' "$output"
        skipped=$((skipped + 1))
        return 0
    fi
    if [[ ! -f "$source" ]]; then
        printf 'MISS  %s (checkpoint not found)\n' "$source" >&2
        failures+=("$source")
        return 0
    fi

    printf 'RUN   %s %s %s -> %s\n' "$task" "$model_code" "$input_size" "$output"
    if [[ "$DRY_RUN" -eq 1 ]]; then
        printf '      '
        printf '%q ' "${command[@]}"
        printf '\n'
        return 0
    fi

    mkdir -p "$OUTPUT_DIR"
    if "${command[@]}"; then
        exported=$((exported + 1))
    else
        printf 'FAIL  %s\n' "$output" >&2
        failures+=("$output")
    fi
}

while (($# > 0)); do
    case "$1" in
        --source-dir)
            (($# >= 2)) || die "--source-dir requires a value"
            SOURCE_DIR="$2"
            shift 2
            ;;
        --output-dir)
            (($# >= 2)) || die "--output-dir requires a value"
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --tasks)
            (($# >= 2)) || die "--tasks requires a value"
            split_csv "$2" TASKS
            shift 2
            ;;
        --models)
            (($# >= 2)) || die "--models requires a value"
            split_csv "$2" MODEL_CODES
            shift 2
            ;;
        --sizes)
            (($# >= 2)) || die "--sizes requires a value"
            split_csv "$2" INPUT_SIZES
            shift 2
            ;;
        --device)
            (($# >= 2)) || die "--device requires a value"
            DEVICE="$2"
            shift 2
            ;;
        --force)
            FORCE=1
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1"
            ;;
    esac
done

validate_lists
printf 'source: %s\n' "$SOURCE_DIR"
printf 'output: %s\n' "$OUTPUT_DIR"
printf 'tasks: %s\n' "${TASKS[*]}"
printf 'models: %s\n' "${MODEL_CODES[*]}"
printf 'sizes: %s\n' "${INPUT_SIZES[*]}"
printf '\n'

for task in "${TASKS[@]}"; do
    for model_code in "${MODEL_CODES[@]}"; do
        for input_size in "${INPUT_SIZES[@]}"; do
            export_one "$task" "$model_code" "$input_size"
        done
    done
done

printf '\nexported: %d, skipped: %d, failed/missing: %d\n' "$exported" "$skipped" "${#failures[@]}"
if ((${#failures[@]} > 0)); then
    printf 'failed or missing inputs:\n' >&2
    printf '  %s\n' "${failures[@]}" >&2
    exit 1
fi
