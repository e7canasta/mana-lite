use super::super::models::{ModelCatalog, ModelEntry, ModelTask};
use super::*;
use std::fs;
use std::path::PathBuf;

#[test]
fn nested_patches_override_only_their_fields() {
    let mut base = ModelPatch {
        confidence: Some(0.25),
        postprocess: Some(PostprocessPatch {
            allow_classes: Some(vec!["person".into()]),
            min_confidence: Some(0.2),
            ..Default::default()
        }),
        ..Default::default()
    };
    let child = ModelPatch {
        postprocess: Some(PostprocessPatch {
            allow_classes: Some(vec!["face".into()]),
            nms_iou: Some(0.05),
            ..Default::default()
        }),
        ..Default::default()
    };

    base.merge(&child);

    let postprocess = base.postprocess.expect("postprocess patch");
    assert_eq!(postprocess.allow_classes, Some(vec!["face".into()]));
    assert_eq!(postprocess.min_confidence, Some(0.2));
    assert_eq!(postprocess.nms_iou, Some(0.05));
    assert_eq!(base.confidence, Some(0.25));
}

#[test]
fn duplicate_model_keys_across_task_files_are_rejected() {
    let root = std::env::temp_dir().join(format!(
        "mana-lite-model-catalog-{}-duplicate",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create temporary catalog directory");
    fs::write(
        root.join("models.toml"),
        "include = [\"one.toml\", \"two.toml\"]\n",
    )
    .expect("write manifest");
    fs::write(
        root.join("one.toml"),
        "task = \"detect\"\n[models.shared]\npath = \"one.onnx\"\n",
    )
    .expect("write first task file");
    fs::write(
        root.join("two.toml"),
        "task = \"pose\"\n[models.shared]\npath = \"two.onnx\"\n",
    )
    .expect("write second task file");

    let error = load_model_catalog(&root.join("models.toml")).unwrap_err();
    assert!(error.to_string().contains("duplicate model key"));

    fs::remove_dir_all(root).expect("remove temporary catalog directory");
}

#[test]
fn overlay_resolves_against_parent_and_merges_nested_fields() {
    let root = std::env::temp_dir().join(format!(
        "mana-lite-model-catalog-{}-overlay",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("blueprint")).expect("create temporary catalog directory");
    fs::write(root.join("models.toml"), "include = [\"detect.toml\"]\n").expect("write manifest");
    fs::write(
            root.join("detect.toml"),
            "task = \"detect\"\n[models.detector]\npath = \"detector.onnx\"\nconfidence = 0.25\n[models.detector.postprocess]\nallow_classes = [\"person\"]\nmin_confidence = 0.2\n",
        )
        .expect("write task file");
    fs::write(
            root.join("blueprint/models.toml"),
            "extends = \"../models.toml\"\n[models.detector]\nconfidence = 0.8\n[models.detector.postprocess]\nallow_classes = [\"face\"]\nmax_detections = 1\n",
        )
        .expect("write overlay");

    let mut catalog = load_model_catalog(&root.join("models.toml")).unwrap();
    let overridden = apply_model_overlay(
        &mut catalog,
        &root.join("blueprint/models.toml"),
        &root.join("models.toml"),
    )
    .unwrap();

    assert_eq!(overridden, vec!["detector"]);
    let detector = &catalog.models["detector"];
    assert_eq!(detector.confidence, 0.8);
    assert_eq!(detector.postprocess.allow_classes, vec!["face"]);
    assert_eq!(detector.postprocess.min_confidence, 0.2);
    assert_eq!(detector.postprocess.max_detections, Some(1));

    fs::remove_dir_all(root).expect("remove temporary catalog directory");
}

fn entry_with_path(path: &str) -> ModelEntry {
    ModelPatch {
        path: Some(PathBuf::from(path)),
        ..Default::default()
    }
    .resolve("test", ModelTask::Detect)
    .expect("a patch carrying a path resolves")
}

/// Los catálogos traen rutas relativas a la raíz del repo y los pesos están
/// gitignoreados. Sin este override la suite solo corre desde un checkout que
/// además tenga los ONNX bajados — no en un worktree, ni en un clone, ni en CI.
#[test]
fn models_home_rebases_only_relative_paths() {
    let mut catalog = ModelCatalog::default();
    catalog.models.insert(
        "relativo".into(),
        entry_with_path("tools/model-tools/artifacts/detect.onnx"),
    );
    catalog
        .models
        .insert("absoluto".into(), entry_with_path("/opt/mana/pinned.onnx"));

    rebase_model_paths(&mut catalog, Some(PathBuf::from("/srv/mana")));

    assert_eq!(
        catalog.models["relativo"].path,
        PathBuf::from("/srv/mana/tools/model-tools/artifacts/detect.onnx"),
        "una ruta relativa se reancla en MANA_MODELS_HOME"
    );
    assert_eq!(
        catalog.models["absoluto"].path,
        PathBuf::from("/opt/mana/pinned.onnx"),
        "una ruta absoluta la fija el despliegue: no se toca"
    );
}

#[test]
fn without_models_home_the_catalog_is_untouched() {
    let mut catalog = ModelCatalog::default();
    catalog.models.insert(
        "relativo".into(),
        entry_with_path("tools/model-tools/artifacts/detect.onnx"),
    );

    rebase_model_paths(&mut catalog, None);

    assert_eq!(
        catalog.models["relativo"].path,
        PathBuf::from("tools/model-tools/artifacts/detect.onnx"),
        "sin override, el comportamiento por defecto no cambia"
    );
}
