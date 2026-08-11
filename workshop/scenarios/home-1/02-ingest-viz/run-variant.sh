#!/usr/bin/env bash
# Corre el escenario 02 con una de sus variantes de transporte.
#
# Existe para que una corrida sea una invocación y no la edición manual de un
# campo: dejar un `image_format` flipeado de la corrida anterior invalida la
# comparación sin que nada lo avise. El único bloque que cambia entre variantes
# es `[viz]`; el resto del escenario se toma de `mana.toml` sin tocarlo.
#
#   ./run-variant.sh a-raw-native   referencia sin comprimir, 6,2 MB/frame
#   ./run-variant.sh b-jpeg-native  comprimido, ~195 KB/frame
set -euo pipefail

VARIANT="${1:-b-jpeg-native}"
DURATION="${2:-90}"

case "$VARIANT" in
  a-raw-native)  FORMAT=raw  ;;
  b-jpeg-native) FORMAT=jpeg ;;
  *) echo "variante desconocida: $VARIANT" >&2; exit 2 ;;
esac

SCENARIO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCENARIO_DIR/../../.." && pwd)"
RUN_DIR="$REPO_ROOT/workshop/runs/02-ingest-viz/$VARIANT"
mkdir -p "$RUN_DIR"

# La config efectiva de la corrida se materializa completa, no por diferencias:
# queda junto a su salida como registro de qué se corrió exactamente.
CONFIG="$RUN_DIR/mana.toml"
python3 - "$SCENARIO_DIR/mana.toml" "$CONFIG" "$FORMAT" "$VARIANT" <<'PY'
import sys
src, dst, fmt, variant = sys.argv[1:5]
s = open(src, encoding='utf-8').read()
s = s.replace('image_format = "jpeg"', f'image_format = "{fmt}"')
s = s.replace(
    'save_dir = "workshop/runs/02-ingest-viz"',
    f'save_dir = "workshop/runs/02-ingest-viz/{variant}"')
s = s.replace(
    'snapshot_dir = "workshop/runs/02-ingest-viz/snapshots"',
    f'snapshot_dir = "workshop/runs/02-ingest-viz/{variant}/snapshots"')
open(dst, 'w', encoding='utf-8').write(s)
PY

echo "== variante $VARIANT — image_format=$FORMAT — ${DURATION}s"
echo "== config efectiva: $CONFIG"
cd "$REPO_ROOT"
timeout "$DURATION" cargo run --release -q -- --config "$CONFIG" 2>&1 \
  | tee "$RUN_DIR/run.log" \
  | grep -E 'viz:|ingest:|cycle:|reconnect' || true

echo
echo "== compuertas"
printf 'viz: connected      %s (esperado: 1)\n' "$(grep -c 'viz: connected' "$RUN_DIR/run.log" || true)"
printf 'reconexiones rtsp   %s (esperado: 0)\n' "$(grep -c 'rtsp reconnect attempt' "$RUN_DIR/run.log" || true)"
printf 'overruns de ciclo   %s (esperado: 0)\n' "$(grep -o '[0-9]* overruns' "$RUN_DIR/run.log" | awk '{s+=$1} END{print s+0}')"
# La comparación es acumulada, no por ventana. Un keyframe visto al final de
# una ventana se emite en la siguiente, así que `processed < seen` en una
# ventana aislada es un straddle de borde, no una pérdida. Lo que delata una
# pérdida real es la deriva acumulada, o una ventana muerta con tráfico.
grep -oE '[0-9]+ keyframes processed \([0-9]+ seen\)' "$RUN_DIR/run.log" \
  | awk '{p=$1; s=$4; gsub(/[(]/,"",s); P+=p; S+=s; if (p+0==0 && s+0>0) dead++}
         END{
           printf "keyframes acumulados   %d procesados / %d vistos (deriva %d, tolerado: <=1)\n", P, S, S-P;
           printf "ventanas muertas       %d (esperado: 0)\n", dead+0;
         }'
