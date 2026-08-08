#!/usr/bin/env bash
set -Eeuo pipefail

# Export YOLO11/YOLO12 face checkpoints for the sizes and input resolutions
# used by the face cascade. The source directory may contain browser-download
# suffixes such as " (1)"; the first matching checkpoint is selected.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd -- "$PROJECT_DIR/../.." && pwd)"

SOURCE_DIR="$REPO_ROOT/models"
OUTPUT_DIR="$PROJECT_DIR/artifacts/yoloface-fp16"
DEVICE="cpu"
DRY_RUN=0
FORCE=0
VERSIONS=(11 12)
MODEL_CODES=(s m l)
INPUT_SIZES=(320 640)

declare -a failures=()
exported=0
skipped=0

usage() {
    cat <<'EOF'
Usage: scripts/export-yoloface-fp16-matrix.sh [options]

Export YOLO11/YOLO12 face checkpoints to FP16 ONNX at 320 and 640.

Options:
  --source-dir DIR   Directory containing YOLO face .pt checkpoints
  --output-dir DIR   Artifact directory (default: artifacts/yoloface-fp16)
  --versions LIST    Comma-separated model versions: 11,12
  --models LIST      Comma-separated model sizes: s,m,l
  --sizes LIST       Comma-separated input sizes (default: 320,640)
  --device DEVICE    Ultralytics device (default: cpu)
  --force            Re-export files that already exist
  --dry-run          Print planned exports without running them
  -h, --help         Show this help

Expected checkpoint names:
  yolov11{s,m,l}-face.pt
  yolov12{s,m,l}-face.pt

Browser-download suffixes such as "yolov12s-face (1).pt" are also accepted.
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
    ((${#destination[@]} > 0)) || die "empty list: $value"
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
    local version model_code input_size
    for version in "${VERSIONS[@]}"; do
        contains "$version" 11 12 || die "unsupported YOLO face version: $version"
    done
    for model_code in "${MODEL_CODES[@]}"; do
        contains "$model_code" s m l || die "unsupported model size: $model_code"
    done
    for input_size in "${INPUT_SIZES[@]}"; do
        [[ "$input_size" =~ ^[0-9]+$ && "$input_size" -gt 0 ]] || die "invalid input size: $input_size"
    done
}

find_checkpoint() {
    local version="$1"
    local model_code="$2"
    local candidate
    local -a candidates=(
        "$SOURCE_DIR/yolov${version}${model_code}-face.pt"
        "$SOURCE_DIR/yolov${version}${model_code}-face (1).pt"
        "$SOURCE_DIR/yolov${version}${model_code}-face (2).pt"
        "$SOURCE_DIR/yolo${version}${model_code}-face.pt"
    )
    for candidate in "${candidates[@]}"; do
        if [[ -f "$candidate" ]]; then
            printf '%s' "$candidate"
            return 0
        fi
    done
    return 1
}

export_one() {
    local version="$1"
    local model_code="$2"
    local input_size="$3"
    local source output
    source="$(find_checkpoint "$version" "$model_code" || true)"
    output="$OUTPUT_DIR/yolov${version}${model_code}-face-fp16-${input_size}.onnx"

    if [[ -z "$source" ]]; then
        printf 'MISS  YOLOv%s%s face checkpoint\n' "$version" "$model_code" >&2
        failures+=("yolov${version}${model_code}-face.pt")
        return 0
    fi
    if [[ -s "$output" && "$FORCE" -eq 0 ]]; then
        printf 'SKIP  %s (already exists)\n' "$output"
        skipped=$((skipped + 1))
        return 0
    fi

    local command=(
        uv run --project "$PROJECT_DIR" model-tools export
        --model "$source"
        --output "$output"
        --imgsz "$input_size"
        --device "$DEVICE"
        --half
    )
    printf 'RUN   %s -> %s\n' "$source" "$output"
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
        --versions)
            (($# >= 2)) || die "--versions requires a value"
            split_csv "$2" VERSIONS
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

for version in "${VERSIONS[@]}"; do
    for model_code in "${MODEL_CODES[@]}"; do
        for input_size in "${INPUT_SIZES[@]}"; do
            export_one "$version" "$model_code" "$input_size"
        done
    done
done

printf '\nexported: %d, skipped: %d, failed/missing: %d\n' "$exported" "$skipped" "${#failures[@]}"
if ((${#failures[@]} > 0)); then
    printf 'failed or missing inputs:\n' >&2
    printf '  %s\n' "${failures[@]}" >&2
    exit 1
fi
