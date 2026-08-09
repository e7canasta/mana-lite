use std::collections::HashMap;

use serde::Deserialize;

use crate::depth_map::DepthFrame;

/// Version del esquema del evento `depth_region`.
pub const DEPTH_REGION_EVENT_VERSION: u8 = 2;

/// Calibracion de escena para una regla (spec §9/§15).
///
/// El modelo depth emite metros relativos al entrenamiento; sin referencia
/// fisica no se debe afirmar distancia metrica calibrada. Una referencia de
/// un solo punto fija la escala: `scene = model * (reference_scene_m /
/// reference_model_m)`. Sin calibracion la escala es identidad (unidades del
/// modelo).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct DepthCalibration {
    pub reference_model_m: f32,
    pub reference_scene_m: f32,
}

impl DepthCalibration {
    /// Factor de escala `scene/model`; identidad si no hay calibracion.
    #[must_use]
    pub fn scale(self) -> f32 {
        self.reference_scene_m / self.reference_model_m
    }
}

/// Metrica de `DepthRoiStats` sobre la que evalúa una regla.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepthMetric {
    Min,
    Median,
    P10,
    P90,
    Max,
}

impl DepthMetric {
    const fn value(self, stats: &DepthRoiStats) -> Option<f32> {
        match self {
            Self::Min => stats.min_depth_m,
            Self::Median => stats.median_depth_m,
            Self::P10 => stats.p10_depth_m,
            Self::P90 => stats.p90_depth_m,
            Self::Max => stats.max_depth_m,
        }
    }
}

/// Operador de comparacion de una regla.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepthOp {
    Lt,
    Gt,
}

/// Regla funcional sin dependencia de modelos (spec §9/§15).
///
/// Consulta una region global contra el mapa local del ROI y compara una
/// metrica robusta con un umbral, emitiendo evidencia numerica sin gatear
/// otros modelos. Con `calibration` opcional, `threshold_m` y `value` se
/// expresan en unidades de escena (escala por referencia de un punto).
#[derive(Debug, Clone, Deserialize)]
pub struct DepthRegionRule {
    pub name: String,
    pub region: [u32; 4],
    #[serde(default = "default_metric")]
    pub metric: DepthMetric,
    #[serde(default = "default_op")]
    pub op: DepthOp,
    pub threshold_m: f32,
    #[serde(default = "default_min_valid_ratio")]
    pub min_valid_ratio: f32,
    #[serde(default)]
    pub calibration: Option<DepthCalibration>,
}

const fn default_metric() -> DepthMetric {
    DepthMetric::Median
}

const fn default_op() -> DepthOp {
    DepthOp::Lt
}

const fn default_min_valid_ratio() -> f32 {
    0.5
}

/// Resultado de evaluar una regla sobre un mapa.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthRuleResult {
    pub rule: String,
    pub region: [u32; 4],
    pub metric: DepthMetric,
    pub threshold_m: f32,
    pub value: Option<f32>,
    pub triggered: bool,
    pub valid_pixels: u64,
    pub valid_ratio: Option<f32>,
    pub calibration: Option<DepthCalibration>,
}

impl DepthRegionRule {
    /// Valida la configuracion de la regla.
    #[must_use]
    pub fn validate(&self) -> Option<String> {
        if self.name.is_empty() {
            return Some("rule name must not be empty".into());
        }
        let [x1, y1, x2, y2] = self.region;
        if x2 <= x1 || y2 <= y1 {
            return Some(format!("rule '{}' has invalid region", self.name));
        }
        if !self.threshold_m.is_finite() || self.threshold_m <= 0.0 {
            return Some(format!("rule '{}' has invalid threshold_m", self.name));
        }
        if !(0.0..=1.0).contains(&self.min_valid_ratio) {
            return Some(format!("rule '{}' has invalid min_valid_ratio", self.name));
        }
        if let Some(cal) = self.calibration
            && (!cal.reference_model_m.is_finite()
                || !cal.reference_scene_m.is_finite()
                || cal.reference_model_m <= 0.0
                || cal.reference_scene_m <= 0.0
                || !cal.scale().is_finite())
        {
            return Some(format!(
                "rule '{}' has invalid calibration references (must be finite and > 0)",
                self.name
            ));
        }
        None
    }

    /// Evalua la regla contra el mapa local al `roi`. `None` si la region no
    /// interseca el ROI o no alcanza `min_valid_ratio` de valores validos.
    ///
    /// Con calibracion, `value` y `threshold_m` estan en unidades de escena;
    /// sin calibracion, en unidades del modelo (identidad).
    #[must_use]
    pub fn evaluate(&self, depth: &DepthFrame, roi: [u32; 4]) -> Option<DepthRuleResult> {
        let stats = region_stats(depth, roi, self.region)?;
        if stats
            .valid_ratio
            .is_some_and(|ratio| ratio < self.min_valid_ratio)
        {
            return None;
        }
        let value = self
            .metric
            .value(&stats)
            .map(|raw| self.calibration.map_or(raw, |cal| raw * cal.scale()));
        let triggered = value.is_some_and(|value| match self.op {
            DepthOp::Lt => value < self.threshold_m,
            DepthOp::Gt => value > self.threshold_m,
        });
        Some(DepthRuleResult {
            rule: self.name.clone(),
            region: self.region,
            metric: self.metric,
            threshold_m: self.threshold_m,
            value,
            triggered,
            valid_pixels: stats.valid_pixels,
            valid_ratio: stats.valid_ratio,
            calibration: self.calibration,
        })
    }
}

/// Catalogo de reglas funcionales de profundidad (spec §9).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DepthRules {
    #[serde(default)]
    pub rules: Vec<DepthRegionRule>,
}

impl DepthRules {
    /// Valida todas las reglas del catalogo (nombres unicos y campos).
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let mut names = std::collections::HashSet::new();
        for rule in &self.rules {
            if let Some(error) = rule.validate() {
                errors.push(error);
            }
            if !names.insert(rule.name.clone()) {
                errors.push(format!("duplicate depth rule name '{}'", rule.name));
            }
        }
        errors
    }

    /// Evalua todas las reglas contra el mapa local al `roi`. Devuelve
    /// solamente las reglas con evidencia valida (interseccion y cobertura
    /// suficientes).
    #[must_use]
    pub fn evaluate(&self, depth: &DepthFrame, roi: [u32; 4]) -> Vec<DepthRuleResult> {
        self.rules
            .iter()
            .filter_map(|rule| rule.evaluate(depth, roi))
            .collect()
    }
}

/// Estado de las reglas depth del ultimo frame con evidencia, para que los
/// guards FSM consulten sin mezclar percepcion con identidad (spec §15).
///
/// `is_triggered` devuelve `None` cuando la regla no tuvo evidencia en el
/// frame (sin mapa, region fuera del ROI o cobertura insuficiente); los
/// guards deben tratar `None` como falso (no hay evidencia -> no se afirma).
#[derive(Debug, Clone, Default)]
pub struct DepthRuleSnapshot {
    results: HashMap<String, DepthRuleResult>,
}

impl DepthRuleSnapshot {
    #[must_use]
    pub fn from_results(results: &[DepthRuleResult]) -> Self {
        Self {
            results: results
                .iter()
                .map(|result| (result.rule.clone(), result.clone()))
                .collect(),
        }
    }

    /// `Some(true/false)` si la regla tuvo evidencia; `None` sin evidencia.
    #[must_use]
    pub fn is_triggered(&self, rule: &str) -> Option<bool> {
        self.results.get(rule).map(|result| result.triggered)
    }
}

/// Estadisticas robustas de una consulta de region sobre el mapa depth
/// local al ROI (spec §7/§8). La region se da en coordenadas globales y se
/// interseca con el ROI; nunca se consulta el frame completo.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthRoiStats {
    pub roi: [u32; 4],
    pub region: [u32; 4],
    pub local: [u32; 4],
    pub map_width: u32,
    pub map_height: u32,
    pub valid_pixels: u64,
    pub valid_ratio: Option<f32>,
    pub min_depth_m: Option<f32>,
    pub median_depth_m: Option<f32>,
    pub p10_depth_m: Option<f32>,
    pub p90_depth_m: Option<f32>,
    pub max_depth_m: Option<f32>,
}

/// Interseccion global->local de una region contra el ROI (§7).
///
/// `roi` y `region` estan en coordenadas globales; el resultado es una caja
/// local al ROI. `None` si no hay interseccion.
#[must_use]
pub fn region_intersection(roi: [u32; 4], region: [u32; 4]) -> Option<[u32; 4]> {
    let ix1 = region[0].max(roi[0]);
    let iy1 = region[1].max(roi[1]);
    let ix2 = region[2].min(roi[2]);
    let iy2 = region[3].min(roi[3]);
    if ix2 <= ix1 || iy2 <= iy1 {
        None
    } else {
        Some([ix1 - roi[0], iy1 - roi[1], ix2 - roi[0], iy2 - roi[1]])
    }
}

/// Estadisticas de profundidad para una region global consultada contra el
/// mapa local al ROI. `None` si la region no interseca el ROI o el mapa no
/// tiene ningun valor valido.
#[must_use]
pub fn region_stats(depth: &DepthFrame, roi: [u32; 4], region: [u32; 4]) -> Option<DepthRoiStats> {
    let mut local = region_intersection(roi, region)?;
    let (map_height, map_width) = map_dims(depth);
    local[2] = local[2].min(map_width);
    local[3] = local[3].min(map_height);
    if local[2] <= local[0] || local[3] <= local[1] {
        return None;
    }

    let mut values: Vec<f32> = Vec::new();
    for y in local[1]..local[3] {
        for x in local[0]..local[2] {
            let value = depth.value_at(y as usize, x as usize);
            if value.is_finite() && value > 0.0 {
                values.push(value);
            }
        }
    }
    if values.is_empty() {
        return None;
    }

    values.sort_unstable_by(f32::total_cmp);
    let area = u64::from(local[2] - local[0]) * u64::from(local[3] - local[1]);
    let n = values.len();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let percentile = |p: f32| {
        let rank = ((p * n as f32).ceil() as usize)
            .saturating_sub(1)
            .min(n - 1);
        values[rank]
    };

    Some(DepthRoiStats {
        roi,
        region,
        local,
        map_width,
        map_height,
        valid_pixels: n as u64,
        #[allow(clippy::cast_precision_loss)]
        valid_ratio: (area > 0).then(|| n as f32 / area as f32),
        min_depth_m: Some(values[0]),
        median_depth_m: Some(percentile(0.5)),
        p10_depth_m: Some(percentile(0.1)),
        p90_depth_m: Some(percentile(0.9)),
        max_depth_m: Some(values[n - 1]),
    })
}

/// Dimensiones del mapa (ancho, alto) en coordenadas locales al ROI.
#[must_use]
pub fn map_dims(depth: &DepthFrame) -> (u32, u32) {
    depth.dims()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;
    use ultralytics_inference::DepthMap;

    #[allow(clippy::cast_possible_truncation)]
    fn map_from_rows(rows: &[&[f32]]) -> DepthFrame {
        let data = Array2::from_shape_fn((rows.len(), rows[0].len()), |(y, x)| rows[y][x]);
        DepthFrame::from_ultralytics(DepthMap::new(
            data,
            (rows.len() as u32, rows[0].len() as u32),
        ))
    }

    #[test]
    fn section7_intersection_example() {
        let roi = [560, 140, 1240, 820];
        let region = [768, 320, 900, 500];
        assert_eq!(region_intersection(roi, region), Some([208, 180, 340, 360]));
    }

    #[test]
    fn fully_outside_region_has_no_depth() {
        let roi = [560, 140, 1240, 820];
        assert_eq!(region_intersection(roi, [0, 0, 100, 100]), None);
    }

    #[test]
    fn stats_median_and_percentiles() {
        let roi = [0, 0, 4, 4];
        let map = map_from_rows(&[
            &[1.0, 2.0, 3.0, 0.0],
            &[4.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0],
        ]);
        let stats = region_stats(&map, roi, [0, 0, 4, 4]).expect("full ROI query");
        assert_eq!(stats.valid_pixels, 4);
        assert_eq!(stats.valid_ratio, Some(0.25));
        assert_eq!(stats.min_depth_m, Some(1.0));
        assert_eq!(stats.max_depth_m, Some(4.0));
        assert_eq!(stats.median_depth_m, Some(2.0));
        assert_eq!(stats.p10_depth_m, Some(1.0));
        assert_eq!(stats.p90_depth_m, Some(4.0));
    }

    #[test]
    fn region_partially_outside_map_is_clamped() {
        let roi = [10, 10, 14, 14];
        let map = map_from_rows(&[
            &[0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 5.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0],
        ]);
        let stats = region_stats(&map, roi, [12, 12, 100, 100]).expect("clamped query");
        assert_eq!(stats.local, [2, 2, 4, 4]);
        assert_eq!(stats.valid_pixels, 1);
        assert_eq!(stats.valid_ratio, Some(0.25));
    }

    #[test]
    fn region_without_valid_pixels_has_no_stats() {
        let roi = [0, 0, 4, 4];
        let map = map_from_rows(&[&[0.0; 4], &[0.0; 4], &[0.0; 4], &[0.0; 4]]);
        assert_eq!(region_stats(&map, roi, [0, 0, 4, 4]), None);
    }

    #[test]
    fn map_dims_from_data_shape() {
        let map = map_from_rows(&[&[1.0, 2.0], &[3.0, 4.0], &[5.0, 6.0]]);
        assert_eq!(map_dims(&map), (2, 3));
    }

    fn rule(
        name: &str,
        region: [u32; 4],
        metric: DepthMetric,
        op: DepthOp,
        threshold: f32,
    ) -> DepthRegionRule {
        DepthRegionRule {
            name: name.into(),
            region,
            metric,
            op,
            threshold_m: threshold,
            min_valid_ratio: 0.0,
            calibration: None,
        }
    }

    #[test]
    fn rule_triggers_below_threshold() {
        let roi = [0, 0, 2, 2];
        let map = map_from_rows(&[&[1.0, 1.0], &[1.0, 1.0]]);
        let depth_rule = rule("close", [0, 0, 2, 2], DepthMetric::Median, DepthOp::Lt, 2.0);
        let result = depth_rule.evaluate(&map, roi).expect("rule evaluated");
        assert!(result.triggered);
        assert_eq!(result.value, Some(1.0));
        assert_eq!(result.valid_ratio, Some(1.0));
    }

    #[test]
    fn rule_does_not_trigger_above_threshold() {
        let roi = [0, 0, 2, 2];
        let map = map_from_rows(&[&[3.0, 3.0], &[3.0, 3.0]]);
        let depth_rule = rule("close", [0, 0, 2, 2], DepthMetric::Median, DepthOp::Lt, 2.0);
        let result = depth_rule.evaluate(&map, roi).expect("rule evaluated");
        assert!(!result.triggered);
    }

    #[test]
    fn rule_suppressed_by_min_valid_ratio() {
        let roi = [0, 0, 2, 2];
        let map = map_from_rows(&[&[1.0, 0.0], &[0.0, 0.0]]);
        let mut depth_rule = rule(
            "sparse",
            [0, 0, 2, 2],
            DepthMetric::Median,
            DepthOp::Lt,
            2.0,
        );
        depth_rule.min_valid_ratio = 0.5;
        assert_eq!(depth_rule.evaluate(&map, roi), None);
    }

    #[test]
    fn rule_outside_roi_has_no_evidence() {
        let roi = [0, 0, 2, 2];
        let map = map_from_rows(&[&[1.0, 1.0], &[1.0, 1.0]]);
        let depth_rule = rule(
            "outside",
            [10, 10, 20, 20],
            DepthMetric::Median,
            DepthOp::Lt,
            2.0,
        );
        assert_eq!(depth_rule.evaluate(&map, roi), None);
    }

    #[test]
    fn rule_validation_rejects_bad_configuration() {
        assert!(
            rule("", [0, 0, 1, 1], DepthMetric::Median, DepthOp::Lt, 1.0)
                .validate()
                .is_some()
        );
        let mut depth_rule = rule("r", [5, 5, 5, 6], DepthMetric::Median, DepthOp::Lt, 1.0);
        assert!(depth_rule.validate().is_some());
        depth_rule.region = [0, 0, 1, 1];
        depth_rule.threshold_m = -1.0;
        assert!(depth_rule.validate().is_some());
    }

    #[test]
    fn depth_rules_reject_duplicate_names() {
        let rules = DepthRules {
            rules: vec![
                rule("dup", [0, 0, 1, 1], DepthMetric::Median, DepthOp::Lt, 1.0),
                rule("dup", [0, 0, 1, 1], DepthMetric::Median, DepthOp::Lt, 1.0),
            ],
        };
        assert_eq!(rules.validate().len(), 1);
    }

    #[test]
    fn depth_rules_evaluate_gt_metric() {
        let rules = DepthRules {
            rules: vec![rule(
                "far",
                [0, 0, 2, 2],
                DepthMetric::Max,
                DepthOp::Gt,
                2.0,
            )],
        };
        let roi = [0, 0, 2, 2];
        let map = map_from_rows(&[&[1.0, 3.0], &[2.0, 2.0]]);
        let results = rules.evaluate(&map, roi);
        assert_eq!(results.len(), 1);
        assert!(results[0].triggered);
        assert_eq!(results[0].value, Some(3.0));
    }

    #[test]
    fn configured_depth_rules_file_is_valid() {
        let content = std::fs::read_to_string("config/depth-rules.toml").expect("depth rules file");
        let rules: DepthRules = toml::from_str(&content).expect("parse depth rules");
        assert!(rules.validate().is_empty());
        assert_eq!(rules.rules.len(), 1);
        assert_eq!(rules.rules[0].name, "bed-approach");
        assert_eq!(rules.rules[0].metric, DepthMetric::Median);
        assert_eq!(rules.rules[0].op, DepthOp::Lt);
    }

    #[test]
    fn calibration_scales_value_before_comparison() {
        let roi = [0, 0, 2, 2];
        let map = map_from_rows(&[&[2.0, 2.0], &[2.0, 2.0]]);
        let mut depth_rule = rule(
            "calibrated",
            [0, 0, 2, 2],
            DepthMetric::Median,
            DepthOp::Lt,
            1.2,
        );
        // Modelo lee 2.0 m; la escena mide 1.0 m -> k = 0.5 -> value = 1.0.
        depth_rule.calibration = Some(DepthCalibration {
            reference_model_m: 2.0,
            reference_scene_m: 1.0,
        });
        let result = depth_rule.evaluate(&map, roi).expect("rule evaluated");
        assert_eq!(result.value, Some(1.0));
        assert!(result.triggered);
        assert_eq!(
            result.calibration,
            Some(DepthCalibration {
                reference_model_m: 2.0,
                reference_scene_m: 1.0
            })
        );
    }

    #[test]
    fn calibration_rejects_invalid_references() {
        let mut depth_rule = rule("cal", [0, 0, 1, 1], DepthMetric::Median, DepthOp::Lt, 1.0);
        depth_rule.calibration = Some(DepthCalibration {
            reference_model_m: 0.0,
            reference_scene_m: 1.0,
        });
        assert!(depth_rule.validate().is_some());
        depth_rule.calibration = Some(DepthCalibration {
            reference_model_m: 2.0,
            reference_scene_m: -1.0,
        });
        assert!(depth_rule.validate().is_some());
    }

    #[test]
    fn snapshot_tracks_triggered_and_evidence() {
        let results = vec![
            DepthRuleResult {
                rule: "close".into(),
                region: [0, 0, 2, 2],
                metric: DepthMetric::Median,
                threshold_m: 2.0,
                value: Some(1.0),
                triggered: true,
                valid_pixels: 4,
                valid_ratio: Some(1.0),
                calibration: None,
            },
            DepthRuleResult {
                rule: "far".into(),
                region: [0, 0, 2, 2],
                metric: DepthMetric::Median,
                threshold_m: 2.0,
                value: Some(3.0),
                triggered: false,
                valid_pixels: 4,
                valid_ratio: Some(1.0),
                calibration: None,
            },
        ];
        let snapshot = DepthRuleSnapshot::from_results(&results);
        assert_eq!(snapshot.is_triggered("close"), Some(true));
        assert_eq!(snapshot.is_triggered("far"), Some(false));
        assert_eq!(snapshot.is_triggered("no-evidence"), None);
        assert!(DepthRuleSnapshot::default().is_triggered("close").is_none());
    }
}
