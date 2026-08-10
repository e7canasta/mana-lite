//! Identifiers and semantics for pipeline domain objects.
//!
//! Prefer these newtypes over bare `String` comparisons against model keys,
//! class names, FSM states, or zone ids.
//!
//! Mechanism: [`mana_id`]. Control port vocabulary: `mana_control::domain`.
//! Perception vocabulary: `mana_perception::{ModelId,ClassName}`. This binary
//! keeps application-facing `ModelId` / `ClassName` for catalog and track
//! surfaces; adapters convert with `.as_str()` at the control port.

use std::collections::HashMap;

use crate::config::{ModelCatalog, ModelTask};

pub use mana_id::DomStr;
use mana_id::domain_id;

domain_id!(ModelId, "Catalog key for an inference model.");
domain_id!(ClassName, "Detection class label (person, face, …).");

/// Rendering capability of a model output.
///
/// Distinct from [`ModelTask`] (ONNX head type). It determines the visual
/// representation, while clinical identity remains catalog-key metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelRole {
    Boxes,
    Mask,
    Skeleton,
    DepthMap,
    Other,
}

impl ModelRole {
    /// Derive a render capability from the model task.
    #[must_use]
    pub const fn from_task(task: ModelTask) -> Self {
        match task {
            ModelTask::Detect => Self::Boxes,
            ModelTask::Pose => Self::Skeleton,
            ModelTask::Segment | ModelTask::Semantic => Self::Mask,
            ModelTask::Depth => Self::DepthMap,
            ModelTask::Classify | ModelTask::Obb => Self::Other,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Boxes => "boxes",
            Self::Mask => "mask",
            Self::Skeleton => "skeleton",
            Self::DepthMap => "depth_map",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSemantics {
    pub task: ModelTask,
    pub role: ModelRole,
}

#[derive(Debug, Clone)]
pub struct RegistryEntry {
    pub semantics: ModelSemantics,
    pub enabled: bool,
}

/// Runtime lookup for model enablement, task, and role.
#[derive(Debug, Clone)]
pub struct ModelRegistry {
    entries: HashMap<ModelId, RegistryEntry>,
    primary: ModelId,
}

impl ModelRegistry {
    #[must_use]
    pub fn from_catalog(catalog: &ModelCatalog, primary: impl Into<ModelId>) -> Self {
        let primary = primary.into();
        let entries = catalog
            .models
            .iter()
            .map(|(name, entry)| {
                let id = ModelId::new(name);
                let role = ModelRole::from_task(entry.task);
                (
                    id,
                    RegistryEntry {
                        semantics: ModelSemantics {
                            task: entry.task,
                            role,
                        },
                        enabled: entry.enabled,
                    },
                )
            })
            .collect();
        Self { entries, primary }
    }

    #[must_use]
    pub fn primary(&self) -> &ModelId {
        &self.primary
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&RegistryEntry> {
        self.entries.get(id)
    }

    #[must_use]
    pub fn enabled(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(|entry| entry.enabled)
    }

    #[must_use]
    pub fn task_of(&self, id: &str) -> Option<ModelTask> {
        self.entries.get(id).map(|entry| entry.semantics.task)
    }

    #[must_use]
    pub fn role_of(&self, id: &str) -> Option<ModelRole> {
        self.entries.get(id).map(|entry| entry.semantics.role)
    }

    #[must_use]
    pub fn is_depth(&self, id: &str) -> bool {
        self.role_of(id) == Some(ModelRole::DepthMap) || self.task_of(id) == Some(ModelTask::Depth)
    }

    #[must_use]
    pub fn is_face_model(&self, id: &str) -> bool {
        id.to_ascii_lowercase().contains("face")
    }

    #[must_use]
    pub fn is_pose(&self, id: &str) -> bool {
        self.role_of(id) == Some(ModelRole::Skeleton)
    }

    #[must_use]
    pub fn is_segment(&self, id: &str) -> bool {
        self.role_of(id) == Some(ModelRole::Mask)
    }

    #[must_use]
    pub fn is_box_model(&self, id: &str) -> bool {
        self.role_of(id) == Some(ModelRole::Boxes)
    }

    #[must_use]
    pub fn first_with_role(&self, role: ModelRole) -> Option<&ModelId> {
        self.entries
            .iter()
            .find(|(_, entry)| entry.enabled && entry.semantics.role == role)
            .map(|(id, _)| id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ModelId, &RegistryEntry)> {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_map_tasks_to_render_capabilities() {
        assert_eq!(ModelRole::from_task(ModelTask::Detect), ModelRole::Boxes);
        assert_eq!(ModelRole::from_task(ModelTask::Pose), ModelRole::Skeleton);
        assert_eq!(ModelRole::from_task(ModelTask::Depth), ModelRole::DepthMap);
        assert_eq!(ModelRole::from_task(ModelTask::Segment), ModelRole::Mask);
    }

    #[test]
    fn model_id_borrows_as_str_for_hashmap() {
        let mut map = HashMap::new();
        map.insert(ModelId::new("detect-fast"), 1u8);
        assert_eq!(map.get("detect-fast"), Some(&1));
    }
}
