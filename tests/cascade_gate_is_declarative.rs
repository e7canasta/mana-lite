//! La compuerta del hijo de la cascada vive en el blueprint, no en el código.
//!
//! Hubo una compuerta hardcodeada en `src/app/inference.rs` que rechazaba al
//! hijo cuando no había exactamente un track de persona. Duplicaba
//! `requires_exact_count = 1`, pero contra otra fuente de clase y sin estar
//! declarada en ningún catálogo. Se borró.
//!
//! Estos tests fijan lo que esa compuerta protegía, contra los blueprints que
//! efectivamente se despliegan: si alguien afloja una regla, esto se cae.

use std::path::Path;

use mana_lite::cascade::{CascadeScheduler, GateObservation};
use mana_lite::config::BlueprintConfig;
use mana_lite::config::load_config;
use mana_perception::domain::{ClassName, ModelId};

const FRAME_W: u32 = 1920;
const FRAME_H: u32 = 1080;

/// Los blueprints de producción que tienen un hijo en la cascada.
const BLUEPRINTS: &[&str] = &[
    "config/blueprints/detect-face/blueprint.toml",
    "config/blueprints/detect-room-face/blueprint.toml",
    "config/blueprints/detect-face-pose-seg/blueprint.toml",
];

fn scheduler(relative: &str) -> (CascadeScheduler, Vec<String>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let blueprint: BlueprintConfig =
        load_config(&root.join(relative)).unwrap_or_else(|err| panic!("cargar {relative}: {err}"));
    let hijos = blueprint
        .rules
        .iter()
        .filter(|rule| rule.requires.is_some())
        .map(|rule| rule.model.clone())
        .collect();
    (
        CascadeScheduler::from_rules_and_regions(&blueprint.rules, blueprint.regions.clone()),
        hijos,
    )
}

fn persona(id: u64, x: f32) -> GateObservation {
    GateObservation {
        id,
        bbox: [x, 200.0, x + 200.0, 700.0],
        class: ClassName::new("person"),
        confidence: 0.93,
        source_model: ModelId::new("detect-fast"),
        is_confirmed: true,
        misses: 0,
    }
}

#[test]
fn con_una_persona_confirmada_el_hijo_tiene_recorte() {
    for relative in BLUEPRINTS {
        let (cascade, hijos) = scheduler(relative);
        assert!(!hijos.is_empty(), "{relative} no declara ningún hijo");
        for hijo in &hijos {
            assert!(
                cascade
                    .target_for(hijo, &[persona(1, 700.0)], FRAME_W, FRAME_H)
                    .is_some(),
                "{relative}: con una persona confirmada, '{hijo}' tiene que recortar"
            );
        }
    }
}

/// Lo que protegía la compuerta borrada. Con dos personas la escena deja de ser
/// una sesión de una sola persona y el hijo no corre: `requires_exact_count`
/// lo rechaza sin que haga falta una línea de código que lo repita.
#[test]
fn con_dos_personas_confirmadas_el_hijo_no_corre() {
    for relative in BLUEPRINTS {
        let (cascade, hijos) = scheduler(relative);
        let dos = [persona(1, 400.0), persona(2, 1100.0)];
        for hijo in &hijos {
            assert!(
                cascade.target_for(hijo, &dos, FRAME_W, FRAME_H).is_none(),
                "{relative}: con dos personas, '{hijo}' no tiene que correr"
            );
        }
    }
}

/// Un track que existe pero perdió su última medición tampoco habilita al hijo:
/// recortar sobre una posición extrapolada es recortar sobre una suposición.
#[test]
fn un_track_sin_medicion_reciente_no_habilita_al_hijo() {
    for relative in BLUEPRINTS {
        let (cascade, hijos) = scheduler(relative);
        let perdido = [GateObservation {
            misses: 1,
            ..persona(1, 700.0)
        }];
        let tentativo = [GateObservation {
            is_confirmed: false,
            ..persona(1, 700.0)
        }];
        for hijo in &hijos {
            assert!(
                cascade
                    .target_for(hijo, &perdido, FRAME_W, FRAME_H)
                    .is_none(),
                "{relative}: '{hijo}' no debe recortar sobre un track sin medición"
            );
            assert!(
                cascade
                    .target_for(hijo, &tentativo, FRAME_W, FRAME_H)
                    .is_none(),
                "{relative}: '{hijo}' no debe recortar sobre un track sin confirmar"
            );
        }
    }
}
