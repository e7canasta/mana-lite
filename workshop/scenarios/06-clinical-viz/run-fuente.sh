#!/usr/bin/env bash
# Corre el escenario 06 contra una de sus fuentes.
#
# Existe por la misma razón que `02/run-variant.sh`: una corrida es una
# invocación y no la edición manual de un campo. Y porque este escenario tiene
# un problema propio — la cámara de la instalación suele estar vacía, y una
# revisión visual de la pila clínica sin nadie en escena no muestra nada.
#
#   ./run-fuente.sh clip1 180    clip local con una persona en cama
#   ./run-fuente.sh home2 180    la cámara de la instalación
#
# Necesita un visor de Rerun escuchando. Por defecto en la estación de trabajo;
# para mirar en esta máquina: RERUN_ADDR=127.0.0.1:9876 ./run-fuente.sh clip1
set -euo pipefail

FUENTE="${1:-clip1}"
DURACION="${2:-180}"

case "$FUENTE" in
  clip1) URL="rtsp://127.0.0.1:8554/clip1"  ;;
  home2) URL="rtsp://192.168.1.6:8554/home2" ;;
  *) echo "fuente desconocida: $FUENTE (clip1 | home2)" >&2; exit 2 ;;
esac

DIR_ESCENARIO="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RAIZ="$(cd "$DIR_ESCENARIO/../../.." && pwd)"
DIR_CORRIDA="$RAIZ/workshop/runs/06-clinical-viz/$FUENTE"
mkdir -p "$DIR_CORRIDA"

# La config efectiva se materializa completa junto a su salida: queda como
# registro de qué se corrió exactamente, no como una diferencia que hay que
# reconstruir después.
CONFIG="$DIR_CORRIDA/mana.toml"
python3 - "$DIR_ESCENARIO/mana.toml" "$CONFIG" "$URL" "$FUENTE" "${RERUN_ADDR:-}" <<'PY'
import sys
src, dst, url, fuente, addr = sys.argv[1:6]
s = open(src, encoding='utf-8').read()
s = s.replace('url = "rtsp://192.168.1.6:8554/home2"', f'url = "{url}"')
if fuente != 'home2':
    # El clip local no pide credenciales; dejarlas mentiría sobre la fuente.
    s = s.replace('username = "admin"\npassword = ""\n', '')
if addr:
    s = s.replace('rerun_addr = "192.168.1.20:9876"', f'rerun_addr = "{addr}"')
s = s.replace('save_dir = "workshop/runs/06-clinical-viz"',
              f'save_dir = "workshop/runs/06-clinical-viz/{fuente}"')
s = s.replace('snapshot_dir = "workshop/runs/06-clinical-viz/snapshots"',
              f'snapshot_dir = "workshop/runs/06-clinical-viz/{fuente}/snapshots"')
open(dst, 'w', encoding='utf-8').write(s)
PY

echo "== fuente $FUENTE — $URL — ${DURACION}s"
echo "== visor:  $(grep -oE 'rerun_addr = "[^"]+"' "$CONFIG")"
echo "== config: $CONFIG"
cd "$RAIZ"
timeout "$DURACION" cargo run --release -q -- --config "$CONFIG" 2>&1 \
  | tee "$DIR_CORRIDA/run.log" \
  | grep -E 'viz:|cycle:|dline:|evid:|ingest:|infer:|face-yolo|detect-fast' || true

echo
echo "== compuertas"
printf 'viz: connected      %s (esperado: 1)\n' "$(grep -c 'viz: connected' "$DIR_CORRIDA/run.log" || true)"
printf 'reconexiones rtsp   %s (esperado: 0)\n' "$(grep -c 'rtsp reconnect attempt' "$DIR_CORRIDA/run.log" || true)"
printf 'overruns de ciclo   %s (esperado: 0)\n' "$(grep -o '[0-9]* overruns' "$DIR_CORRIDA/run.log" | awk '{s+=$1} END{print s+0}')"
# El costo del visor sobre la pila completa. No es pasa/no pasa: es el número
# que este escenario produce, y se compara contra el 05 corriendo sin visor.
grep -oE 'late min [0-9.]+ms p50 [0-9.]+ms p95 [0-9.]+ms max [0-9.]+ms' "$DIR_CORRIDA/run.log" \
  | awk '{if ($9+0 > m) m=$9+0} END{printf "atraso del lazo     %.1fms peor caso (05 sin visor: 6,4ms)\n", m}'
# `viz_pisados` no es una compuerta: el visor recibe **muestras**, y que se
# pise una significa que el bridge no llegó a tomarla antes de la siguiente.
# Es la degradación correcta —el lazo no espera al visor— y es el segundo
# número que este escenario produce. Los otros dos sí son compuertas: si
# percepción o control empiezan a pisar, el visor está costando evidencia.
printf 'viz_pisados         %s (cuántas muestras no llegaron al visor)\n' \
  "$(grep -oE 'viz_pisados:[0-9]+' "$DIR_CORRIDA/run.log" | awk -F: '{s+=$2} END{print s+0}')"
for f in kf_pisados img_pisadas; do
  printf '%-19s %s (esperado: 0)\n' "$f" \
    "$(grep -oE "$f:[0-9]+" "$DIR_CORRIDA/run.log" | awk -F: '{s+=$2} END{print s+0}')"
done
printf 'transiciones fsm    %s\n' "$(grep -hc '"type":"fsm"' "$DIR_CORRIDA"/*.jsonl 2>/dev/null | awk '{s+=$1} END{print s+0}')"
