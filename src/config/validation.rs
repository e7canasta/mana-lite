use super::models::ModelCatalog;

pub fn validate_model_catalog(models: &ModelCatalog) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, entry) in &models.models {
        if !entry.is_valid() {
            errors.push(format!(
                "models.{name}: confidence, iou, max_det and imgsz must be valid positive model settings"
            ));
        }
        if !entry.postprocess.is_valid() {
            errors.push(format!(
                "models.{name}.postprocess: confidence and area thresholds must be finite and ordered"
            ));
        }
        if entry.crop.as_ref().is_some_and(|crop| !crop.is_valid()) {
            errors.push(format!(
                "models.{name}.crop: square_size must be positive and upper_fraction must be within 0..=1"
            ));
        }
    }
    errors
}
