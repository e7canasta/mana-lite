#!/usr/bin/env bash
set -euo pipefail

BASE="http://127.0.0.1:1984/api/frame.jpeg"
SRC="${1:-home2}"
COUNT="${2:-10}"
INTERVAL="${3:-1}"
OUTDIR="${4:-}"
PREFIX="frame"
URL="${BASE}?src=${SRC}"

if [ -z "$OUTDIR" ]; then
  RUNS_DIR="runs"
  LAST=$(ls -d "$RUNS_DIR"/run_* 2>/dev/null | sed 's/.*run_//' | sort -n | tail -1) || true
  NEXT=$(( ${LAST:-0} + 1 ))
  OUTDIR=$(printf "%s/run_%03d" "$RUNS_DIR" "$NEXT")
fi

mkdir -p "$OUTDIR"

for i in $(seq 1 "$COUNT"); do
  printf -v idx "%04d" "$i"
  OUT="${OUTDIR}/${PREFIX}_${idx}.jpeg"
  curl -s -o "$OUT" "$URL" || { echo "Error capturando frame $i" >&2; exit 1; }
  SIZE=$(stat -c%s "$OUT")
  echo "Capturado: $OUT (${SIZE} bytes)"
  if [ "$i" -lt "$COUNT" ]; then
    sleep "$INTERVAL"
  fi
done

echo "Done: $COUNT frames en $OUTDIR"
