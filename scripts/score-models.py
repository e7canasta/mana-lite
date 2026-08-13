#!/usr/bin/env python3
"""Score de modelos depth del workshop (demo-deep-calib/results/*.json).

Geometría de cámara: cámara montada en la pared de los PIES de la cama,
picada hacia la cabecera → la cabecera (head) es el extremo MÁS LEJANO y los
pies el más cercano. Gradiente esperado: bed/feet < bed/body < bed/head.

Tests de aceptación (criterio del taller):

  Test A (sentado — flip a azul): cara + ambos hombros deben leer zona `bed`.
      Al incorporarse, la persona se separa de la pared de la cabecera y el
      modelo bueno la lee sobre la cama: cara y hombros en bed/head, el cuerpo
      fluye bed/body → bed/feet.
  Test B (acostado — extremo lejano): la cara debe leer MÁS LEJOS que la
      mediana observada de bed/head (zona floor / amarilla). La cabeza
      acostada queda contra la pared de la cabecera: leer lejos es lo
      esperado físicamente (ángulo + foco + profundidad).

Un modelo pasa si cumple AMBOS: el mismo cuerpo partido, acostado lee lejos
(amarillo) y sentado lee sobre la cama (azul).

Salida: matriz por parte (zona) por pose + veredicto por modelo.
"""

import glob
import json
import sys

RESULTS = "demo-deep-calib/results"
SHOULDERS = ("left_shoulder", "right_shoulder")
BED_ZONES = ("bed/head", "bed/body", "bed/feet")

def load(img, model):
    with open(f"{RESULTS}/{img}.{model}.json") as fh:
        return json.load(fh)

def part_zones(run):
    return {k["part"]: (k["zone"] or "none") for k in run["keypoints"]}

def face_zone(run):
    face = run.get("face")
    return face["zone"] if face else "none"

def face_depth(run):
    face = run.get("face")
    return face["depth_m"] if face else None

def bed_head_observed(run):
    for zone in run["zones"]:
        if zone["zone"] == "bed/head":
            return zone["observed_m"]
    return None

def test_sentado(run):
    """A: cara + hombros sobre la cama (azul)."""
    kp = part_zones(run)
    if face_zone(run) not in BED_ZONES:
        return False
    return all(kp.get(p) in BED_ZONES for p in SHOULDERS)

def test_acostado(run):
    """B: cara más lejos que bed/head observada (amarillo)."""
    face = face_depth(run)
    bed_head = bed_head_observed(run)
    return face is not None and bed_head is not None and face > bed_head

def main():
    models = sorted(
        {
            path.split("/")[-1].split(".")[-2].replace("depth-", "")
            for path in glob.glob(f"{RESULTS}/*.json")
        }
    )
    print("== Matriz por parte (zona) ==")
    for img in ("sentado-1", "acostado-1"):
        print(f"\n--- {img} ---")
        print(
            f"{'model':<13} {'face':<10} {'should_L':<9} {'should_R':<9} "
            f"{'hip_L':<9} {'hip_R':<9} {'knee_L':<9} {'knee_R':<9}"
        )
        for model in models:
            run = load(img, f"depth-{model}")
            kp = part_zones(run)
            z = lambda p: (kp.get(p) or "-")[:9]
            print(
                f"{model:<13} {face_zone(run):<10} {z('left_shoulder'):<9} "
                f"{z('right_shoulder'):<9} {z('left_hip'):<9} {z('right_hip'):<9} "
                f"{z('left_knee'):<9} {z('right_knee'):<9}"
            )

    print("\n== Veredicto ==")
    print(f"{'model':<13} {'A sentado (azul)':<18} {'B acostado (lejos)':<20} veredicto")
    for model in models:
        a = test_sentado(load("sentado-1", f"depth-{model}"))
        b = test_acostado(load("acostado-1", f"depth-{model}"))
        verdict = "ok" if a and b else "DESCARTAR"
        print(
            f"{model:<13} {'si' if a else 'NO':<18} {'si' if b else 'NO':<20} {verdict}"
        )

if __name__ == "__main__":
    sys.exit(main())
