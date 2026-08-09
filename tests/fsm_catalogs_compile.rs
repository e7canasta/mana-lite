//! Boot-parity compile of every FSM catalog in the repo.

use std::collections::HashSet;
use std::path::Path;

use mana_lite::config::{load_depth_rules, load_fsm_catalog, load_model_catalog, load_zone_catalog};
use mana_lite::fsm::FsmProgram;

const FSM_PATHS: &[&str] = &[
    "config/fsm.toml",
    "config/blueprints/detect-room-face/fsm.toml",
];

#[test]
fn all_fsm_catalogs_compile_with_references() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let zones = load_zone_catalog(&root.join("config/zones.toml")).expect("zones");
    let models = load_model_catalog(&root.join("config/models.toml")).expect("models");
    let depth_rules = load_depth_rules(&root.join("config/depth-rules.toml")).expect("depth");
    let depth_names: HashSet<String> = depth_rules.rules.iter().map(|r| r.name.clone()).collect();

    for relative in FSM_PATHS {
        let path = root.join(relative);
        let catalog = load_fsm_catalog(&path).unwrap_or_else(|err| {
            panic!("load {relative}: {err}");
        });
        FsmProgram::compile_with_references(
            &catalog,
            Some(&zones),
            &models,
            Some(&depth_names),
        )
        .unwrap_or_else(|errors| {
            panic!("{relative} failed compile_with_references: {errors:?}");
        });
    }
}
