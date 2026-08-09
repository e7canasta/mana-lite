//! Identifiers and semantics for pipeline domain objects.
//!
//! Prefer these newtypes over bare `String` comparisons against model keys,
//! class names, FSM states, or zone ids.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

use crate::config::{ModelCatalog, ModelTask};

/// Shared string newtype used by domain identifiers.
#[derive(Clone, Eq)]
pub struct DomStr(Arc<str>);

impl DomStr {
    #[must_use]
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(Arc::from(value.as_ref()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for DomStr {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl PartialEq<str> for DomStr {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for DomStr {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl Hash for DomStr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl Deref for DomStr {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for DomStr {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for DomStr {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for DomStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for DomStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for DomStr {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for DomStr {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

macro_rules! domain_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Clone, PartialEq, Eq, Hash)]
        pub struct $name(DomStr);

        impl $name {
            #[must_use]
            pub fn new(value: impl AsRef<str>) -> Self {
                Self(DomStr::new(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl Deref for $name {
            type Target = str;
            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(stringify!($name))
                    .field(&self.as_str())
                    .finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.as_str() == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }
    };
}

domain_id!(ModelId, "Catalog key for an inference model.");
domain_id!(ClassName, "Detection class label (person, face, …).");
domain_id!(StateId, "FSM state identifier from the catalog.");
domain_id!(ZoneId, "Spatial zone identifier from the catalog.");

impl StateId {
    /// Structural safe state used after panics / data loss.
    pub const BLIND: &'static str = "blind";
}

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
