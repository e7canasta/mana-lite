use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::domain::{ClassName, ModelId};

/// TTL máximo permitido para una solicitud urgente.
pub const MAX_INFERENCE_REQUEST_TTL: Duration = Duration::from_secs(5);

/// Solicitud one-shot de ejecución fuera del intervalo normal de un modelo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceRequest {
    pub model_key: String,
    pub reason: String,
    pub priority: u8,
    pub requested_at: Instant,
    pub expires_at: Instant,
}

impl InferenceRequest {
    #[must_use]
    pub fn new(
        model_key: impl Into<String>,
        reason: impl Into<String>,
        priority: u8,
        requested_at: Instant,
        expires_at: Instant,
    ) -> Self {
        Self {
            model_key: model_key.into(),
            reason: reason.into(),
            priority,
            requested_at,
            expires_at,
        }
    }

    fn key(&self) -> RequestKey {
        RequestKey {
            model_key: self.model_key.clone(),
            reason: self.reason.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceRequestError {
    EmptyModel,
    EmptyReason,
    ExpirationNotAfterRequest,
    TtlExceedsMaximum,
    RequestedInFuture,
    UnknownModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RequestKey {
    model_key: String,
    reason: String,
}

/// Vista congelada de las urgencias al comienzo de un keyframe.
#[derive(Debug, Default)]
pub struct UrgentRequestWindow {
    pub requests: Vec<InferenceRequest>,
    pub expired: Vec<InferenceRequest>,
    pub starved: Vec<InferenceRequest>,
}

/// Narrow port for cascade gating — no SORT filter state.
#[derive(Debug, Clone, PartialEq)]
pub struct GateObservation {
    pub id: u64,
    pub bbox: [f32; 4],
    pub class: ClassName,
    pub confidence: f32,
    pub source_model: ModelId,
    pub is_confirmed: bool,
    pub misses: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CascadeRule {
    pub model: String,
    pub requires: Option<String>,
    pub requires_class: Option<String>,
    #[serde(default)]
    pub requires_exact_count: Option<usize>,
    #[serde(default)]
    pub same_frame: bool,
    #[serde(default)]
    pub requires_min_confidence: Option<f32>,
    #[serde(default)]
    pub requires_min_area_ratio: Option<f32>,
    #[serde(default)]
    pub requires_region: Option<String>,
    #[serde(default)]
    pub requires_region_coverage: Option<f32>,
    /// Minimum time between starts. Zero means every eligible keyframe.
    #[serde(default)]
    pub interval_min_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SemanticRegion {
    pub rect: [f32; 4],
    #[serde(default)]
    #[allow(dead_code)]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CascadeConfig {
    pub rules: Vec<CascadeRule>,
    #[serde(default)]
    pub regions: HashMap<String, SemanticRegion>,
}

impl CascadeConfig {
    /// Validate cascade rules against a name→enabled map (no ModelCatalog).
    pub fn validate(&self, models: &HashMap<String, bool>, primary_model: &str) -> Vec<String> {
        let mut errors = Vec::new();
        self.validate_primary(models, primary_model, &mut errors);
        for rule in &self.rules {
            self.validate_rule_refs(rule, models, primary_model, &mut errors);
            self.validate_rule_thresholds(rule, &mut errors);
        }
        self.validate_regions(&mut errors);
        errors
    }

    fn validate_primary(
        &self,
        _models: &HashMap<String, bool>,
        primary_model: &str,
        errors: &mut Vec<String>,
    ) {
        let primary_rule = self.rules.iter().find(|rule| rule.model == primary_model);
        if primary_rule.is_none() {
            errors.push(format!(
                "primary model '{}' is missing from cascade rules",
                primary_model
            ));
        } else if primary_rule.is_some_and(|rule| rule.requires.is_some()) {
            errors.push(format!(
                "primary model '{}' must be a cascade root",
                primary_model
            ));
        }
    }

    fn validate_rule_refs(
        &self,
        rule: &CascadeRule,
        models: &HashMap<String, bool>,
        primary_model: &str,
        errors: &mut Vec<String>,
    ) {
        if !models.contains_key(&rule.model) {
            errors.push(format!("rule references unknown model '{}'", rule.model));
        }
        if let Some(parent) = &rule.requires {
            if !models.contains_key(parent) {
                errors.push(format!(
                    "model '{}' requires unknown parent '{}'",
                    rule.model, parent
                ));
            }
            let disabled_branch = models.get(parent).is_some_and(|enabled| !*enabled)
                && models.get(&rule.model).is_some_and(|enabled| !*enabled);
            if parent != primary_model && !disabled_branch {
                errors.push(format!(
                    "model '{}' requires '{}', but only primary model '{}' can gate children",
                    rule.model, parent, primary_model,
                ));
            }
        }
        if let Some(region) = &rule.requires_region {
            if !self.regions.contains_key(region) {
                errors.push(format!(
                    "model '{}' references unknown region '{}'",
                    rule.model, region
                ));
            }
        }
    }

    fn validate_rule_thresholds(&self, rule: &CascadeRule, errors: &mut Vec<String>) {
        if rule.requires_exact_count == Some(0) {
            errors.push(format!(
                "model '{}' has invalid requires_exact_count",
                rule.model
            ));
        }
        if rule
            .requires_min_confidence
            .is_some_and(|v| !(0.0..=1.0).contains(&v))
        {
            errors.push(format!(
                "model '{}' has invalid requires_min_confidence",
                rule.model
            ));
        }
        if rule
            .requires_min_area_ratio
            .is_some_and(|v| !(0.0..=1.0).contains(&v))
        {
            errors.push(format!(
                "model '{}' has invalid requires_min_area_ratio",
                rule.model
            ));
        }
        if rule
            .requires_region_coverage
            .is_some_and(|v| !(0.0..=1.0).contains(&v))
        {
            errors.push(format!(
                "model '{}' has invalid requires_region_coverage",
                rule.model
            ));
        }
    }

    fn validate_regions(&self, errors: &mut Vec<String>) {
        for (name, region) in &self.regions {
            let [x1, y1, x2, y2] = region.rect;
            if !(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite())
                || x2 <= x1
                || y2 <= y1
            {
                errors.push(format!("region '{}' has invalid rect", name));
            }
        }
    }
}

struct CascadeEntry {
    requires: Option<String>,
    requires_class: Option<String>,
    requires_exact_count: Option<usize>,
    same_frame: bool,
    requires_min_confidence: Option<f32>,
    requires_min_area_ratio: Option<f32>,
    requires_region: Option<String>,
    requires_region_coverage: Option<f32>,
    interval_min_ms: u64,
    last_started_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CascadeTarget {
    pub id: Option<u64>,
    pub bbox: [f32; 4],
}

/// Timing observed when a model is admitted by the cooperative scheduler.
///
/// `gap` is measured between actual starts. `due_late` is only meaningful for
/// a positive interval and measures the delay after the model's next due time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CascadeStartTiming {
    pub interval_min_ms: u64,
    pub gap: Option<std::time::Duration>,
    pub due_late: Option<std::time::Duration>,
}

pub struct CascadeScheduler {
    entries: HashMap<String, CascadeEntry>,
    order: Vec<String>,
    regions: HashMap<String, SemanticRegion>,
    persistent_requests: Vec<InferenceRequest>,
    persistent_keys: HashSet<RequestKey>,
    transient_requests: VecDeque<InferenceRequest>,
    pending_keyframes: HashMap<RequestKey, u32>,
    starvation_reported: HashSet<RequestKey>,
}

impl CascadeScheduler {
    #[allow(dead_code)]
    pub fn from_rules(rules: &[CascadeRule]) -> Self {
        Self::from_rules_and_regions(rules, HashMap::new())
    }

    pub fn from_rules_and_regions(
        rules: &[CascadeRule],
        regions: HashMap<String, SemanticRegion>,
    ) -> Self {
        let mut entries = HashMap::new();
        let mut order = Vec::new();
        for rule in rules {
            order.push(rule.model.clone());
            entries.insert(
                rule.model.clone(),
                CascadeEntry {
                    requires: rule.requires.clone(),
                    requires_class: rule.requires_class.clone(),
                    requires_exact_count: rule.requires_exact_count,
                    same_frame: rule.same_frame,
                    requires_min_confidence: rule.requires_min_confidence,
                    requires_min_area_ratio: rule.requires_min_area_ratio,
                    requires_region: rule.requires_region.clone(),
                    requires_region_coverage: rule.requires_region_coverage,
                    interval_min_ms: rule.interval_min_ms,
                    last_started_at: None,
                },
            );
        }
        Self {
            entries,
            order,
            regions,
            persistent_requests: Vec::new(),
            persistent_keys: HashSet::new(),
            transient_requests: VecDeque::new(),
            pending_keyframes: HashMap::new(),
            starvation_reported: HashSet::new(),
        }
    }

    pub fn all_models(&self) -> &[String] {
        &self.order
    }

    /// Topo-ordered view of `requested` without owning the names:
    /// roots (no `requires`) first, then children, preserving the
    /// configured order inside each group.
    pub fn ordered<'a>(&self, requested: &'a [String]) -> Vec<&'a str> {
        let mut roots: Vec<&'a str> = Vec::new();
        let mut children: Vec<&'a str> = Vec::new();

        for name in requested {
            match self.entries.get(name.as_str()) {
                Some(entry) if entry.requires.is_none() => roots.push(name.as_str()),
                _ => children.push(name.as_str()),
            }
        }

        roots.extend(children);
        roots
    }

    pub fn parent_of(&self, model: &str) -> Option<&str> {
        self.entries.get(model)?.requires.as_deref()
    }

    pub fn same_frame(&self, model: &str) -> bool {
        self.entries
            .get(model)
            .is_some_and(|entry| entry.same_frame)
    }

    /// Returns whether a model may start on this scheduler tick.
    ///
    /// This is a cooperative interval, not a deadline. A late model is run at
    /// most once when the next fresh keyframe reaches the perception stage; the
    /// scheduler never catches up with a burst of invocations.
    pub fn is_due(&self, model: &str, now: Instant) -> bool {
        let Some(entry) = self.entries.get(model) else {
            return false;
        };
        let Some(last_started_at) = entry.last_started_at else {
            return true;
        };
        entry.interval_min_ms == 0
            || now.saturating_duration_since(last_started_at).as_millis()
                >= u128::from(entry.interval_min_ms)
    }

    /// Records the start of an actual model attempt and returns its timing.
    ///
    /// The timestamp is committed before the backend call so a failing model
    /// cannot be retried on every incoming keyframe in a tight loop.
    pub fn mark_started(&mut self, model: &str, now: Instant) -> Option<CascadeStartTiming> {
        let entry = self.entries.get_mut(model)?;
        let gap = entry
            .last_started_at
            .map(|last_started_at| now.saturating_duration_since(last_started_at));
        let due_late = gap
            .filter(|_| entry.interval_min_ms > 0)
            .map(|gap| gap.saturating_sub(std::time::Duration::from_millis(entry.interval_min_ms)));
        let timing = CascadeStartTiming {
            interval_min_ms: entry.interval_min_ms,
            gap,
            due_late,
        };
        entry.last_started_at = Some(now);
        Some(timing)
    }

    pub fn interval_min_ms(&self, model: &str) -> Option<u64> {
        self.entries.get(model).map(|entry| entry.interval_min_ms)
    }

    /// Valida la parte del contrato que pertenece al scheduler T1.
    ///
    /// La habilitación efectiva y las tareas desactivadas se validan en la
    /// etapa de percepción, que es la única que conoce el catálogo cargado y
    /// `PerceptionConfig`.
    pub fn validate_request(
        &self,
        request: &InferenceRequest,
        now: Instant,
    ) -> Result<(), InferenceRequestError> {
        if request.model_key.trim().is_empty() {
            return Err(InferenceRequestError::EmptyModel);
        }
        if request.reason.trim().is_empty() {
            return Err(InferenceRequestError::EmptyReason);
        }
        if request.expires_at <= request.requested_at {
            return Err(InferenceRequestError::ExpirationNotAfterRequest);
        }
        if request.expires_at.duration_since(request.requested_at) > MAX_INFERENCE_REQUEST_TTL {
            return Err(InferenceRequestError::TtlExceedsMaximum);
        }
        if request.requested_at > now {
            return Err(InferenceRequestError::RequestedInFuture);
        }
        if !self.entries.contains_key(&request.model_key) {
            return Err(InferenceRequestError::UnknownModel);
        }
        Ok(())
    }

    /// Reemplaza el conjunto de requests persistentes de la directiva.
    ///
    /// El identity key permanece activo mientras la directiva lo publique,
    /// incluso después de consumir o expirar la request. Así una directiva
    /// reconstruida en cada scan no revive la misma urgencia one-shot.
    pub fn replace_persistent_requests(
        &mut self,
        requests: Vec<InferenceRequest>,
        now: Instant,
    ) -> Vec<InferenceRequest> {
        let old_keys = std::mem::take(&mut self.persistent_keys);
        let mut next_keys = HashSet::new();
        let mut next_requests = Vec::new();
        for request in requests {
            if self.validate_request(&request, now).is_err() {
                continue;
            }
            let key = request.key();
            if next_keys.insert(key) {
                next_requests.push(request);
            }
        }

        let retained_keys: HashSet<RequestKey> =
            old_keys.intersection(&next_keys).cloned().collect();
        self.persistent_requests
            .retain(|request| next_keys.contains(&request.key()));

        for key in old_keys.difference(&next_keys) {
            self.clear_wait_state(key);
        }

        let mut pending_keys: HashSet<RequestKey> = self
            .persistent_requests
            .iter()
            .map(InferenceRequest::key)
            .collect();
        pending_keys.extend(self.transient_requests.iter().map(InferenceRequest::key));

        let mut accepted = Vec::new();
        for request in next_requests {
            let key = request.key();
            if retained_keys.contains(&key) || pending_keys.contains(&key) {
                continue;
            }
            pending_keys.insert(key.clone());
            self.reset_wait_state(&key);
            self.persistent_requests.push(request.clone());
            accepted.push(request);
        }
        self.persistent_keys = next_keys;
        accepted
    }

    /// Agrega una request transitoria a la cola durable del scheduler.
    ///
    /// `Ok(false)` significa que la request era un duplicado lógico de otra
    /// pendiente o de una urgencia persistente activa.
    pub fn enqueue_transient(
        &mut self,
        request: InferenceRequest,
        now: Instant,
    ) -> Result<bool, InferenceRequestError> {
        self.validate_request(&request, now)?;
        let key = request.key();
        if self.persistent_keys.contains(&key)
            || self
                .persistent_requests
                .iter()
                .any(|pending| pending.key() == key)
            || self
                .transient_requests
                .iter()
                .any(|pending| pending.key() == key)
        {
            return Ok(false);
        }
        self.reset_wait_state(&key);
        self.transient_requests.push_back(request);
        Ok(true)
    }

    /// Congela las requests que pueden competir en el keyframe actual.
    ///
    /// Las entradas producidas después de esta llamada quedan en la cola y no
    /// se observan hasta el siguiente keyframe.
    pub fn begin_keyframe(&mut self, now: Instant) -> UrgentRequestWindow {
        let mut expired = Vec::new();
        let mut expired_keys = HashSet::new();
        self.persistent_requests.retain(|request| {
            if request.expires_at <= now {
                expired.push(request.clone());
                expired_keys.insert(request.key());
                false
            } else {
                true
            }
        });
        self.transient_requests.retain(|request| {
            if request.expires_at <= now {
                expired.push(request.clone());
                expired_keys.insert(request.key());
                false
            } else {
                true
            }
        });
        for key in expired_keys {
            self.clear_wait_state(&key);
        }

        let mut requests = self.persistent_requests.clone();
        requests.extend(self.transient_requests.iter().cloned());
        requests.sort_by(compare_requests);

        let mut starved = Vec::new();
        for request in &requests {
            let key = request.key();
            let keyframes = self.pending_keyframes.entry(key.clone()).or_default();
            *keyframes = keyframes.saturating_add(1);
            if *keyframes >= 2 && self.starvation_reported.insert(key) {
                starved.push(request.clone());
            }
        }

        UrgentRequestWindow {
            requests,
            expired,
            starved,
        }
    }

    /// Consume una request al marcar el inicio, antes del backend.
    pub fn consume_request(&mut self, request: &InferenceRequest) -> bool {
        let removed = self
            .persistent_requests
            .iter()
            .position(|candidate| candidate == request)
            .map(|index| {
                self.persistent_requests.remove(index);
                true
            })
            .or_else(|| {
                self.transient_requests
                    .iter()
                    .position(|candidate| candidate == request)
                    .map(|index| {
                        self.transient_requests.remove(index);
                        true
                    })
            })
            .unwrap_or(false);
        if removed {
            self.clear_wait_state(&request.key());
        }
        removed
    }

    pub fn pending_request_count(&self) -> usize {
        self.persistent_requests.len() + self.transient_requests.len()
    }

    fn reset_wait_state(&mut self, key: &RequestKey) {
        self.pending_keyframes.insert(key.clone(), 0);
        self.starvation_reported.remove(key);
    }

    fn clear_wait_state(&mut self, key: &RequestKey) {
        self.pending_keyframes.remove(key);
        self.starvation_reported.remove(key);
    }

    pub fn target_for_detections(
        &self,
        model: &str,
        detections: &[crate::detection::Detection],
        frame_w: u32,
        frame_h: u32,
    ) -> Option<CascadeTarget> {
        let entry = self.entries.get(model)?;
        if !entry.same_frame || entry.requires.is_none() {
            return None;
        }

        let candidates: Vec<&crate::detection::Detection> = detections
            .iter()
            .filter(|detection| {
                entry
                    .requires_class
                    .as_deref()
                    .is_none_or(|class| detection.class == class)
            })
            .filter(|detection| {
                entry
                    .requires_min_confidence
                    .is_none_or(|min| detection.confidence >= min)
            })
            .filter(|detection| {
                entry
                    .requires_min_area_ratio
                    .is_none_or(|min| bbox_area_ratio(&detection.bbox, frame_w, frame_h) >= min)
            })
            .filter(|detection| {
                entry.requires_region.as_deref().is_none_or(|region_name| {
                    let Some(region) = self.regions.get(region_name) else {
                        return false;
                    };
                    entry.requires_region_coverage.is_none_or(|min| {
                        bbox_region_coverage(&detection.bbox, &region.rect) >= min
                    })
                })
            })
            .collect();
        if entry
            .requires_exact_count
            .is_some_and(|count| count != candidates.len())
        {
            return None;
        }

        candidates
            .into_iter()
            .max_by(|a, b| {
                bbox_area(&a.bbox)
                    .partial_cmp(&bbox_area(&b.bbox))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|detection| CascadeTarget {
                id: None,
                bbox: detection.bbox,
            })
    }

    pub fn target_for(
        &self,
        model: &str,
        observations: &[GateObservation],
        frame_w: u32,
        frame_h: u32,
    ) -> Option<CascadeTarget> {
        let Some(entry) = self.entries.get(model) else {
            return None;
        };

        if entry.requires.is_none() {
            return None;
        }

        let parent_key = entry.requires.as_deref().unwrap_or_default();
        let required_class = entry.requires_class.as_deref();
        let exact_count = entry.requires_exact_count;
        let min_confidence = entry.requires_min_confidence;
        let min_area_ratio = entry.requires_min_area_ratio;
        let required_region = entry.requires_region.as_deref();
        let min_region_coverage = entry.requires_region_coverage;

        let candidates: Vec<&GateObservation> = observations
            .iter()
            .filter(|obs| obs.is_confirmed && obs.misses == 0)
            .filter(|obs| obs.source_model.as_str() == parent_key)
            .filter(|obs| required_class.is_none_or(|class| obs.class.as_str() == class))
            .filter(|obs| min_confidence.is_none_or(|min| obs.confidence >= min))
            .filter(|obs| {
                min_area_ratio.is_none_or(|min| bbox_area_ratio(&obs.bbox, frame_w, frame_h) >= min)
            })
            .filter(|obs| {
                required_region.is_none_or(|region_name| {
                    let Some(region) = self.regions.get(region_name) else {
                        return false;
                    };
                    min_region_coverage
                        .is_none_or(|min| bbox_region_coverage(&obs.bbox, &region.rect) >= min)
                })
            })
            .collect();
        if exact_count.is_some_and(|count| count != candidates.len()) {
            return None;
        }

        let best = candidates.into_iter().max_by(|a, b| {
            bbox_area(&a.bbox)
                .partial_cmp(&bbox_area(&b.bbox))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let Some(obs) = best else {
            return None;
        };

        Some(CascadeTarget {
            id: Some(obs.id),
            bbox: obs.bbox,
        })
    }
}

fn bbox_area(bbox: &[f32; 4]) -> f32 {
    (bbox[2] - bbox[0]).max(0.0) * (bbox[3] - bbox[1]).max(0.0)
}

fn compare_requests(a: &InferenceRequest, b: &InferenceRequest) -> std::cmp::Ordering {
    b.priority
        .cmp(&a.priority)
        .then_with(|| a.requested_at.cmp(&b.requested_at))
        .then_with(|| a.model_key.cmp(&b.model_key))
        .then_with(|| a.reason.cmp(&b.reason))
}

fn bbox_area_ratio(bbox: &[f32; 4], frame_w: u32, frame_h: u32) -> f32 {
    let frame_area = (frame_w as f32) * (frame_h as f32);
    if frame_area <= 0.0 {
        0.0
    } else {
        bbox_area(bbox) / frame_area
    }
}

fn bbox_region_coverage(bbox: &[f32; 4], region: &[f32; 4]) -> f32 {
    let ix1 = bbox[0].max(region[0]);
    let iy1 = bbox[1].max(region[1]);
    let ix2 = bbox[2].min(region[2]);
    let iy2 = bbox[3].min(region[3]);
    let intersection = (ix2 - ix1).max(0.0) * (iy2 - iy1).max(0.0);
    let area = bbox_area(bbox);
    if area <= 0.0 {
        0.0
    } else {
        intersection / area
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn gate_obs(
        id: u64,
        source_model: &str,
        class: &str,
        bbox: [f32; 4],
        confidence: f32,
        is_confirmed: bool,
        misses: u32,
    ) -> GateObservation {
        GateObservation {
            id: id,
            bbox,
            class: ClassName::new(class),
            confidence,
            source_model: ModelId::new(source_model),
            is_confirmed,
            misses,
        }
    }

    fn test_rules() -> Vec<CascadeRule> {
        vec![
            CascadeRule {
                model: "detect-fast".into(),
                requires: None,
                requires_class: None,
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
                interval_min_ms: 0,
            },
            CascadeRule {
                model: "pose-standard".into(),
                requires: Some("detect-fast".into()),
                requires_class: Some("person".into()),
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
                interval_min_ms: 0,
            },
        ]
    }

    fn urgent_request(
        model: &str,
        reason: &str,
        priority: u8,
        requested_at: Instant,
        ttl: Duration,
    ) -> InferenceRequest {
        InferenceRequest::new(model, reason, priority, requested_at, requested_at + ttl)
    }

    #[test]
    fn same_frame_child_requires_exact_parent_count() {
        let cascade = CascadeScheduler::from_rules(&[
            CascadeRule {
                model: "detect-fast".into(),
                requires: None,
                requires_class: None,
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
                interval_min_ms: 0,
            },
            CascadeRule {
                model: "face-yolo".into(),
                requires: Some("detect-fast".into()),
                requires_class: Some("person".into()),
                requires_exact_count: Some(1),
                same_frame: true,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
                interval_min_ms: 0,
            },
        ]);
        let one_person = [crate::detection::Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [0.0, 0.0, 100.0, 100.0],
            keypoints: None,
            mask: None,
        }];
        let two_people = [
            crate::detection::Detection {
                class: "person".into(),
                confidence: 0.9,
                bbox: [0.0, 0.0, 100.0, 100.0],
                keypoints: None,
                mask: None,
            },
            crate::detection::Detection {
                class: "person".into(),
                confidence: 0.9,
                bbox: [120.0, 0.0, 220.0, 100.0],
                keypoints: None,
                mask: None,
            },
        ];
        assert!(
            cascade
                .target_for_detections("face-yolo", &one_person, 640, 480)
                .is_some()
        );
        assert!(
            cascade
                .target_for_detections("face-yolo", &two_people, 640, 480)
                .is_none()
        );
    }

    #[test]
    fn same_frame_child_applies_confidence_and_area_gates() {
        let cascade = CascadeScheduler::from_rules(&[
            CascadeRule {
                model: "detect-fast".into(),
                requires: None,
                requires_class: None,
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
                interval_min_ms: 0,
            },
            CascadeRule {
                model: "face-yolo".into(),
                requires: Some("detect-fast".into()),
                requires_class: Some("person".into()),
                requires_exact_count: Some(1),
                same_frame: true,
                requires_min_confidence: Some(0.8),
                requires_min_area_ratio: Some(0.1),
                requires_region: None,
                requires_region_coverage: None,
                interval_min_ms: 0,
            },
        ]);
        let low_confidence = [crate::detection::Detection {
            class: "person".into(),
            confidence: 0.7,
            bbox: [0.0, 0.0, 640.0, 480.0],
            keypoints: None,
            mask: None,
        }];
        let small = [crate::detection::Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [0.0, 0.0, 100.0, 100.0],
            keypoints: None,
            mask: None,
        }];
        assert!(
            cascade
                .target_for_detections("face-yolo", &low_confidence, 640, 480)
                .is_none()
        );
        assert!(
            cascade
                .target_for_detections("face-yolo", &small, 640, 480)
                .is_none()
        );
    }

    #[test]
    fn roots_before_children() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let requested: Vec<String> = vec!["pose-standard".into(), "detect-fast".into()];
        let result = cascade.ordered(&requested);
        assert_eq!(result, vec!["detect-fast", "pose-standard"]);
    }

    #[test]
    fn interval_is_due_before_first_run() {
        let cascade = CascadeScheduler::from_rules(&[CascadeRule {
            model: "seg-standard".into(),
            requires: None,
            requires_class: None,
            requires_exact_count: None,
            same_frame: false,
            requires_min_confidence: None,
            requires_min_area_ratio: None,
            requires_region: None,
            requires_region_coverage: None,
            interval_min_ms: 2_000,
        }]);
        let start = Instant::now();

        assert!(cascade.is_due("seg-standard", start));
    }

    #[test]
    fn interval_is_not_due_until_minimum_elapsed() {
        let mut cascade = CascadeScheduler::from_rules(&[CascadeRule {
            model: "seg-standard".into(),
            requires: None,
            requires_class: None,
            requires_exact_count: None,
            same_frame: false,
            requires_min_confidence: None,
            requires_min_area_ratio: None,
            requires_region: None,
            requires_region_coverage: None,
            interval_min_ms: 2_000,
        }]);
        let start = Instant::now();
        cascade.mark_started("seg-standard", start);

        assert!(!cascade.is_due("seg-standard", start + Duration::from_millis(1_999)));
        assert!(cascade.is_due("seg-standard", start + Duration::from_millis(2_000)));
    }

    #[test]
    fn start_timing_reports_real_gap_and_lateness_after_next_due() {
        let mut cascade = CascadeScheduler::from_rules(&[CascadeRule {
            model: "seg-standard".into(),
            requires: None,
            requires_class: None,
            requires_exact_count: None,
            same_frame: false,
            requires_min_confidence: None,
            requires_min_area_ratio: None,
            requires_region: None,
            requires_region_coverage: None,
            interval_min_ms: 2_000,
        }]);
        let start = Instant::now();

        let first = cascade.mark_started("seg-standard", start).unwrap();
        assert_eq!(first.interval_min_ms, 2_000);
        assert_eq!(first.gap, None);
        assert_eq!(first.due_late, None);

        let second = cascade
            .mark_started("seg-standard", start + Duration::from_millis(2_350))
            .unwrap();
        assert_eq!(second.gap, Some(Duration::from_millis(2_350)));
        assert_eq!(second.due_late, Some(Duration::from_millis(350)));
    }

    #[test]
    fn a_late_model_has_one_due_run_without_catch_up() {
        let mut cascade = CascadeScheduler::from_rules(&[CascadeRule {
            model: "seg-standard".into(),
            requires: None,
            requires_class: None,
            requires_exact_count: None,
            same_frame: false,
            requires_min_confidence: None,
            requires_min_area_ratio: None,
            requires_region: None,
            requires_region_coverage: None,
            interval_min_ms: 2_000,
        }]);
        let start = Instant::now();
        cascade.mark_started("seg-standard", start);
        let late = start + Duration::from_secs(10);

        assert!(cascade.is_due("seg-standard", late));
        cascade.mark_started("seg-standard", late);
        assert!(!cascade.is_due("seg-standard", late + Duration::from_millis(1)));
    }

    #[test]
    fn zero_interval_preserves_every_keyframe_behavior() {
        let mut cascade = CascadeScheduler::from_rules(&[CascadeRule {
            model: "detect-fast".into(),
            requires: None,
            requires_class: None,
            requires_exact_count: None,
            same_frame: false,
            requires_min_confidence: None,
            requires_min_area_ratio: None,
            requires_region: None,
            requires_region_coverage: None,
            interval_min_ms: 0,
        }]);
        let start = Instant::now();
        cascade.mark_started("detect-fast", start);

        assert!(cascade.is_due("detect-fast", start));
        assert!(cascade.is_due("detect-fast", start + Duration::from_millis(1)));
    }

    #[test]
    fn urgent_request_contract_rejects_invalid_time_and_unknown_model() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let now = Instant::now();

        assert_eq!(
            cascade.validate_request(
                &InferenceRequest::new("pose-standard", "face-uncertain", 1, now, now),
                now,
            ),
            Err(InferenceRequestError::ExpirationNotAfterRequest)
        );
        assert_eq!(
            cascade.validate_request(
                &urgent_request(
                    "pose-standard",
                    "face-uncertain",
                    1,
                    now,
                    MAX_INFERENCE_REQUEST_TTL + Duration::from_millis(1),
                ),
                now,
            ),
            Err(InferenceRequestError::TtlExceedsMaximum)
        );
        assert_eq!(
            cascade.validate_request(
                &urgent_request("missing", "test", 1, now, Duration::from_secs(1)),
                now,
            ),
            Err(InferenceRequestError::UnknownModel)
        );
    }

    #[test]
    fn persistent_requests_are_deduplicated_and_sorted_by_priority() {
        let mut cascade = CascadeScheduler::from_rules(&test_rules());
        let now = Instant::now();
        let older = urgent_request(
            "pose-standard",
            "face-uncertain",
            1,
            now - Duration::from_millis(100),
            Duration::from_secs(1),
        );
        let higher = urgent_request(
            "detect-fast",
            "operator",
            5,
            now - Duration::from_millis(50),
            Duration::from_secs(1),
        );
        let duplicate = urgent_request(
            "pose-standard",
            "face-uncertain",
            9,
            now - Duration::from_millis(10),
            Duration::from_secs(1),
        );

        let accepted = cascade
            .replace_persistent_requests(vec![older.clone(), higher.clone(), duplicate], now);
        assert_eq!(accepted.len(), 2);

        let window = cascade.begin_keyframe(now);
        assert_eq!(window.requests, vec![higher.clone(), older.clone()]);
        assert!(cascade.consume_request(&higher));
        assert!(!cascade.consume_request(&higher));
        assert_eq!(cascade.pending_request_count(), 1);

        let accepted_again = cascade.replace_persistent_requests(vec![older], now);
        assert!(
            accepted_again.is_empty(),
            "la directiva no revive una urgente consumida"
        );
    }

    #[test]
    fn transient_request_survives_persistent_replacement_and_expires_once() {
        let mut cascade = CascadeScheduler::from_rules(&test_rules());
        let now = Instant::now();
        let request = urgent_request(
            "pose-standard",
            "synthetic",
            2,
            now - Duration::from_millis(100),
            Duration::from_secs(1),
        );
        assert!(
            cascade
                .enqueue_transient(request.clone(), now)
                .expect("request is valid")
        );
        assert!(
            !cascade
                .enqueue_transient(request.clone(), now)
                .expect("duplicate is still valid")
        );
        cascade.replace_persistent_requests(Vec::new(), now);
        assert_eq!(cascade.begin_keyframe(now).requests, vec![request]);

        let expired = urgent_request(
            "pose-standard",
            "expired",
            1,
            now - Duration::from_millis(200),
            Duration::from_millis(100),
        );
        assert!(
            cascade
                .enqueue_transient(expired, now)
                .expect("expired request can be observed and counted")
        );
        let first = cascade.begin_keyframe(now);
        assert_eq!(first.expired.len(), 1);
        assert!(cascade.begin_keyframe(now).expired.is_empty());
    }

    #[test]
    fn requests_published_after_freeze_wait_for_the_next_keyframe() {
        let mut cascade = CascadeScheduler::from_rules(&test_rules());
        let now = Instant::now();
        let first = cascade.begin_keyframe(now);
        let request = urgent_request(
            "pose-standard",
            "produced-during-inference",
            1,
            now,
            Duration::from_secs(1),
        );

        cascade
            .enqueue_transient(request.clone(), now)
            .expect("request is valid");
        assert!(first.requests.is_empty());
        assert_eq!(cascade.begin_keyframe(now).requests, vec![request]);
    }

    #[test]
    fn urgent_request_does_not_bypass_a_child_gate() {
        let mut cascade = CascadeScheduler::from_rules(&[
            CascadeRule {
                model: "detect-fast".into(),
                requires: None,
                requires_class: None,
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
                interval_min_ms: 0,
            },
            CascadeRule {
                model: "pose-standard".into(),
                requires: Some("detect-fast".into()),
                requires_class: Some("person".into()),
                requires_exact_count: Some(1),
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
                interval_min_ms: 2_000,
            },
        ]);
        let now = Instant::now();
        cascade.mark_started("pose-standard", now);
        cascade
            .enqueue_transient(
                urgent_request(
                    "pose-standard",
                    "synthetic",
                    1,
                    now,
                    Duration::from_secs(1),
                ),
                now,
            )
            .expect("request is valid");

        let window = cascade.begin_keyframe(now + Duration::from_millis(500));
        assert!(!cascade.is_due("pose-standard", now + Duration::from_millis(500)));
        assert_eq!(window.requests.len(), 1);
        assert!(cascade.target_for("pose-standard", &[], 640, 480).is_none());
    }

    #[test]
    fn pending_request_is_reported_as_starved_once_after_two_keyframes() {
        let mut cascade = CascadeScheduler::from_rules(&test_rules());
        let now = Instant::now();
        let request = urgent_request("pose-standard", "synthetic", 1, now, Duration::from_secs(1));
        cascade
            .enqueue_transient(request, now)
            .expect("request is valid");

        assert!(cascade.begin_keyframe(now).starved.is_empty());
        assert_eq!(
            cascade
                .begin_keyframe(now + Duration::from_millis(10))
                .starved
                .len(),
            1
        );
        assert!(
            cascade
                .begin_keyframe(now + Duration::from_millis(20))
                .starved
                .is_empty()
        );
    }

    #[test]
    fn root_always_runs() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        assert!(cascade.parent_of("detect-fast").is_none());
    }

    #[test]
    fn child_skipped_without_parent() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        assert!(cascade.target_for("pose-standard", &[], 640, 480).is_none());
    }

    #[test]
    fn child_skipped_when_parent_has_no_matching_class() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let obs = gate_obs(
            1,
            "detect-fast",
            "chair",
            [0.0, 0.0, 100.0, 100.0],
            0.9,
            true,
            0,
        );
        assert!(
            cascade
                .target_for("pose-standard", &[obs], 640, 480)
                .is_none()
        );
    }

    #[test]
    fn child_runs_when_parent_has_required_class() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let obs = gate_obs(
            1,
            "detect-fast",
            "person",
            [0.0, 0.0, 100.0, 100.0],
            0.9,
            true,
            0,
        );
        assert!(
            cascade
                .target_for("pose-standard", &[obs], 640, 480)
                .is_some()
        );
    }

    #[test]
    fn unconfirmed_observation_cannot_activate_child() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let obs = gate_obs(
            1,
            "detect-fast",
            "person",
            [0.0, 0.0, 100.0, 100.0],
            0.9,
            false,
            0,
        );
        assert!(
            cascade
                .target_for("pose-standard", &[obs], 640, 480)
                .is_none()
        );
    }

    #[test]
    fn unknown_model_does_not_run() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        assert!(cascade.target_for("nonexistent", &[], 640, 480).is_none());
    }

    #[test]
    fn region_coverage_filters_target() {
        let rules = vec![
            CascadeRule {
                model: "detect-fast".into(),
                requires: None,
                requires_class: None,
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
                interval_min_ms: 0,
            },
            CascadeRule {
                model: "pose-standard".into(),
                requires: Some("detect-fast".into()),
                requires_class: Some("person".into()),
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: Some(0.5),
                requires_min_area_ratio: Some(0.01),
                requires_region: Some("bed".into()),
                requires_region_coverage: Some(0.5),
                interval_min_ms: 0,
            },
        ];
        let regions = HashMap::from([(
            "bed".into(),
            SemanticRegion {
                rect: [0.0, 0.0, 100.0, 100.0],
                label: None,
            },
        )]);
        let cascade = CascadeScheduler::from_rules_and_regions(&rules, regions);
        let obs = gate_obs(
            1,
            "detect-fast",
            "person",
            [50.0, 0.0, 150.0, 100.0],
            0.9,
            true,
            0,
        );
        assert!(
            cascade
                .target_for("pose-standard", &[obs.clone()], 200, 100)
                .is_some()
        );
        assert_eq!(
            cascade
                .target_for("pose-standard", &[obs], 200, 100)
                .unwrap()
                .id,
            Some(1)
        );
    }

    #[test]
    fn validate_accepts_primary_root_with_enabled_map() {
        let config = CascadeConfig {
            rules: test_rules(),
            regions: HashMap::new(),
        };
        let models = HashMap::from([("detect-fast".into(), true), ("pose-standard".into(), true)]);
        assert!(config.validate(&models, "detect-fast").is_empty());
    }

    #[test]
    fn validate_rejects_unknown_model_and_non_root_primary() {
        let config = CascadeConfig {
            rules: test_rules(),
            regions: HashMap::new(),
        };
        let models = HashMap::from([("detect-fast".into(), true)]);
        let errors = config.validate(&models, "pose-standard");
        assert!(errors.iter().any(|e| e.contains("must be a cascade root")));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("unknown model 'pose-standard'"))
        );
    }
}
