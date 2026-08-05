use std::collections::HashMap;
use std::time::Instant;

use serde::Deserialize;

use crate::infer::Detection;

#[derive(Debug, Clone, Deserialize)]
pub struct CascadeRule {
    pub model: String,
    pub requires: Option<String>,
    pub requires_class: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CascadeConfig {
    pub rules: Vec<CascadeRule>,
}

struct CascadeEntry {
    requires: Option<String>,
    requires_class: Option<String>,
    last_run_at: Instant,
}

pub struct CascadeScheduler {
    entries: HashMap<String, CascadeEntry>,
}

impl CascadeScheduler {
    pub fn from_rules(rules: &[CascadeRule]) -> Self {
        let mut entries = HashMap::new();
        for rule in rules {
            entries.insert(rule.model.clone(), CascadeEntry {
                requires: rule.requires.clone(),
                requires_class: rule.requires_class.clone(),
                last_run_at: Instant::now(),
            });
        }
        Self { entries }
    }

    pub fn all_models(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    pub fn ordered(&self, requested: &[String]) -> Vec<String> {
        let mut roots: Vec<String> = Vec::new();
        let mut children: Vec<String> = Vec::new();

        for name in requested {
            match self.entries.get(name) {
                Some(entry) if entry.requires.is_none() => roots.push(name.clone()),
                _ => children.push(name.clone()),
            }
        }

        roots.extend(children);
        roots
    }

    pub fn should_run(
        &mut self,
        model: &str,
        parent_dets: &HashMap<String, Vec<Detection>>,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(model) else {
            return false;
        };

        let Some(ref parent_key) = entry.requires else {
            entry.last_run_at = Instant::now();
            return true;
        };

        let Some(ref req_class) = entry.requires_class else {
            entry.last_run_at = Instant::now();
            return true;
        };

        let Some(parent) = parent_dets.get(parent_key) else {
            return false;
        };

        let found = parent.iter().any(|d| &d.class == req_class);
        if found {
            entry.last_run_at = Instant::now();
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rules() -> Vec<CascadeRule> {
        vec![
            CascadeRule { model: "detect-fast".into(), requires: None, requires_class: None },
            CascadeRule { model: "pose-standard".into(), requires: Some("detect-fast".into()), requires_class: Some("person".into()) },
        ]
    }

    #[test]
    fn roots_before_children() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let requested: Vec<String> = vec!["pose-standard".into(), "detect-fast".into()];
        let result = cascade.ordered(&requested);
        assert_eq!(result, vec!["detect-fast", "pose-standard"]);
    }

    #[test]
    fn root_always_runs() {
        let mut cascade = CascadeScheduler::from_rules(&test_rules());
        let empty: HashMap<String, Vec<Detection>> = HashMap::new();
        assert!(cascade.should_run("detect-fast", &empty));
    }

    #[test]
    fn child_skipped_without_parent() {
        let mut cascade = CascadeScheduler::from_rules(&test_rules());
        let empty: HashMap<String, Vec<Detection>> = HashMap::new();
        assert!(!cascade.should_run("pose-standard", &empty));
    }

    #[test]
    fn child_skipped_when_parent_has_no_matching_class() {
        let mut cascade = CascadeScheduler::from_rules(&test_rules());
        let mut dets = HashMap::new();
        dets.insert("detect-fast".into(), vec![
            Detection {
                class: "chair".into(),
                confidence: 0.9,
                bbox: [0.0; 4],
            },
        ]);
        assert!(!cascade.should_run("pose-standard", &dets));
    }

    #[test]
    fn child_runs_when_parent_has_required_class() {
        let mut cascade = CascadeScheduler::from_rules(&test_rules());
        let mut dets = HashMap::new();
        dets.insert("detect-fast".into(), vec![
            Detection {
                class: "person".into(),
                confidence: 0.9,
                bbox: [0.0; 4],
            },
        ]);
        assert!(cascade.should_run("pose-standard", &dets));
    }

    #[test]
    fn unknown_model_does_not_run() {
        let mut cascade = CascadeScheduler::from_rules(&test_rules());
        let empty: HashMap<String, Vec<Detection>> = HashMap::new();
        assert!(!cascade.should_run("nonexistent", &empty));
    }
}
