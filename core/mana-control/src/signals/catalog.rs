//! Static scene-signal catalog (v1).
//!
//! The catalog is compiled into the producer binary. It is not loaded from
//! deployment TOML and does not hot-reload.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use crate::domain::SignalTag;

use super::SignalKind;

/// When a declared signal has a value in a control-cycle snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalPresence {
    /// The producer emits a value on every cycle.
    Always,
    /// The producer emits a value only when a face was selected.
    WhenFaceSelected,
    /// The producer emits a value only when face/pose validation ran.
    WhenFacePoseValidation,
    /// The producer emits a value only when the dwell ROI is configured.
    WhenDwellRoiConfigured,
    /// The producer emits a value while the FSM is active.
    WhileFsmActive,
}

/// Descriptor for one declared signal tag.
#[derive(Debug, Clone)]
pub struct SignalDescriptor {
    kind: SignalKind,
    presence: SignalPresence,
    allowed_labels: BTreeSet<String>,
}

impl SignalDescriptor {
    #[must_use]
    pub fn kind(&self) -> SignalKind {
        self.kind
    }

    /// Presence semantics for this signal in a cycle snapshot.
    #[must_use]
    pub fn presence(&self) -> SignalPresence {
        self.presence
    }

    /// Closed label set for [`SignalKind::Label`]; empty for other kinds.
    #[must_use]
    pub fn allowed_labels(&self) -> &BTreeSet<String> {
        &self.allowed_labels
    }

    /// Whether `label` is in the closed set for this descriptor.
    #[must_use]
    pub fn allows_label(&self, label: &str) -> bool {
        self.allowed_labels.contains(label)
    }
}

/// Versioned, ordered catalog of declared scene signals.
#[derive(Debug, Clone)]
pub struct SignalCatalog {
    version: u32,
    descriptors: BTreeMap<SignalTag, SignalDescriptor>,
}

impl SignalCatalog {
    /// Catalog schema version. v1 is the initial scene-signal vocabulary.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Number of declared tags.
    #[must_use]
    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }

    /// Whether `tag` is a declared catalog entry.
    ///
    /// Wrapping a string in [`SignalTag`] is not enough: the tag must appear here.
    #[must_use]
    pub fn contains(&self, tag: &SignalTag) -> bool {
        self.descriptors.contains_key(tag)
    }

    /// Descriptor for a declared tag, if any.
    #[must_use]
    pub fn get(&self, tag: &SignalTag) -> Option<&SignalDescriptor> {
        self.descriptors.get(tag)
    }

    /// Stable iteration over declared tags and descriptors (BTree order).
    pub fn iter(&self) -> impl Iterator<Item = (&SignalTag, &SignalDescriptor)> {
        self.descriptors.iter()
    }

    /// Validates the `dominio.atributo` naming convention.
    ///
    /// Rules: exactly one `.`, both sides non-empty, lowercase ASCII letters,
    /// digits and underscores only; the first character of each side is a letter.
    #[must_use]
    pub fn is_valid_tag_name(name: &str) -> bool {
        let Some((domain, attribute)) = name.split_once('.') else {
            return false;
        };
        if attribute.contains('.') {
            return false;
        }
        is_valid_name_part(domain) && is_valid_name_part(attribute)
    }
}

fn is_valid_name_part(part: &str) -> bool {
    let mut chars = part.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn descriptor(kind: SignalKind, presence: SignalPresence, labels: &[&str]) -> SignalDescriptor {
    SignalDescriptor {
        kind,
        presence,
        allowed_labels: labels.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn build_v1() -> SignalCatalog {
    let mut descriptors = BTreeMap::new();
    let entries: [(&str, SignalDescriptor); 11] = [
        (
            "persona.presente",
            descriptor(SignalKind::Bool, SignalPresence::Always, &[]),
        ),
        (
            "persona.cantidad",
            descriptor(SignalKind::Count, SignalPresence::Always, &[]),
        ),
        (
            "cara.presente",
            descriptor(SignalKind::Bool, SignalPresence::Always, &[]),
        ),
        (
            "cara.confianza",
            descriptor(SignalKind::Ratio, SignalPresence::WhenFaceSelected, &[]),
        ),
        (
            "cara.pose_calidad",
            descriptor(
                SignalKind::Ratio,
                SignalPresence::WhenFacePoseValidation,
                &[],
            ),
        ),
        (
            "cara.pose_validada",
            descriptor(
                SignalKind::Bool,
                SignalPresence::WhenFacePoseValidation,
                &[],
            ),
        ),
        (
            "cara.en_dwell",
            descriptor(
                SignalKind::Bool,
                SignalPresence::WhenDwellRoiConfigured,
                &[],
            ),
        ),
        (
            "cara.en_borde",
            descriptor(SignalKind::Bool, SignalPresence::Always, &[]),
        ),
        (
            "cara.modelo_corrio",
            descriptor(SignalKind::Bool, SignalPresence::Always, &[]),
        ),
        (
            "ocupacion.cardinalidad",
            descriptor(
                SignalKind::Label,
                SignalPresence::Always,
                &["empty", "single", "multiple"],
            ),
        ),
        (
            "cara.estuvo_dentro",
            descriptor(SignalKind::Bool, SignalPresence::WhileFsmActive, &[]),
        ),
    ];

    for (name, desc) in entries {
        assert!(
            SignalCatalog::is_valid_tag_name(name),
            "catalog tag `{name}` must match dominio.atributo"
        );
        assert!(
            descriptors.insert(SignalTag::new(name), desc).is_none(),
            "catalog tag `{name}` must be unique"
        );
    }

    SignalCatalog {
        version: 1,
        descriptors,
    }
}

/// Static scene-signal catalog used by the control producer.
#[must_use]
pub fn scene_signal_catalog() -> &'static SignalCatalog {
    static CATALOG: OnceLock<SignalCatalog> = OnceLock::new();
    CATALOG.get_or_init(build_v1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_v1_has_eleven_tags_version_one() {
        let cat = scene_signal_catalog();
        assert_eq!(cat.version(), 1);
        assert_eq!(cat.len(), 11);
        assert!(!cat.is_empty());
    }

    #[test]
    fn catalog_v1_kinds_and_labels() {
        let cat = scene_signal_catalog();
        let expected: [(&str, SignalKind, SignalPresence, &[&str]); 11] = [
            (
                "persona.presente",
                SignalKind::Bool,
                SignalPresence::Always,
                &[],
            ),
            (
                "persona.cantidad",
                SignalKind::Count,
                SignalPresence::Always,
                &[],
            ),
            (
                "cara.presente",
                SignalKind::Bool,
                SignalPresence::Always,
                &[],
            ),
            (
                "cara.confianza",
                SignalKind::Ratio,
                SignalPresence::WhenFaceSelected,
                &[],
            ),
            (
                "cara.pose_calidad",
                SignalKind::Ratio,
                SignalPresence::WhenFacePoseValidation,
                &[],
            ),
            (
                "cara.pose_validada",
                SignalKind::Bool,
                SignalPresence::WhenFacePoseValidation,
                &[],
            ),
            (
                "cara.en_dwell",
                SignalKind::Bool,
                SignalPresence::WhenDwellRoiConfigured,
                &[],
            ),
            (
                "cara.en_borde",
                SignalKind::Bool,
                SignalPresence::Always,
                &[],
            ),
            (
                "cara.modelo_corrio",
                SignalKind::Bool,
                SignalPresence::Always,
                &[],
            ),
            (
                "ocupacion.cardinalidad",
                SignalKind::Label,
                SignalPresence::Always,
                &["empty", "multiple", "single"],
            ),
            (
                "cara.estuvo_dentro",
                SignalKind::Bool,
                SignalPresence::WhileFsmActive,
                &[],
            ),
        ];

        for (name, kind, presence, labels) in expected {
            let tag = SignalTag::new(name);
            let desc = cat.get(&tag).unwrap_or_else(|| panic!("missing {name}"));
            assert_eq!(desc.kind(), kind, "{name}");
            assert_eq!(desc.presence(), presence, "{name} presence");
            let got: Vec<&str> = desc.allowed_labels().iter().map(String::as_str).collect();
            assert_eq!(got, labels, "{name} labels");
        }
    }

    #[test]
    fn wrapping_string_in_signal_tag_does_not_declare_it() {
        let cat = scene_signal_catalog();
        let freestyle = SignalTag::new("zona.ocupada");
        assert!(!cat.contains(&freestyle));
        assert!(cat.get(&freestyle).is_none());
        assert!(cat.contains(&SignalTag::new("persona.presente")));
    }

    #[test]
    fn tag_name_convention() {
        assert!(SignalCatalog::is_valid_tag_name("persona.presente"));
        assert!(SignalCatalog::is_valid_tag_name("cara.en_dwell"));
        assert!(SignalCatalog::is_valid_tag_name("ocupacion.cardinalidad"));
        assert!(!SignalCatalog::is_valid_tag_name(""));
        assert!(!SignalCatalog::is_valid_tag_name("persona"));
        assert!(!SignalCatalog::is_valid_tag_name(".presente"));
        assert!(!SignalCatalog::is_valid_tag_name("persona."));
        assert!(!SignalCatalog::is_valid_tag_name("Persona.presente"));
        assert!(!SignalCatalog::is_valid_tag_name("persona.Presente"));
        assert!(!SignalCatalog::is_valid_tag_name("persona.en-dwell"));
        assert!(!SignalCatalog::is_valid_tag_name("a.b.c"));
        assert!(!SignalCatalog::is_valid_tag_name("1persona.x"));
        assert!(!SignalCatalog::is_valid_tag_name("persona.1bad")); // attribute must start with letter
        assert!(SignalCatalog::is_valid_tag_name("persona.x1"));
    }

    #[test]
    fn cardinality_labels_are_closed() {
        let cat = scene_signal_catalog();
        let desc = cat
            .get(&SignalTag::new("ocupacion.cardinalidad"))
            .expect("cardinality");
        assert!(desc.allows_label("empty"));
        assert!(desc.allows_label("single"));
        assert!(desc.allows_label("multiple"));
        assert!(!desc.allows_label("full"));
        assert!(!desc.allows_label("Empty"));
    }

    #[test]
    fn catalog_iteration_is_sorted() {
        let cat = scene_signal_catalog();
        let names: Vec<&str> = cat.iter().map(|(t, _)| t.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert_eq!(names.len(), 11);
    }

    #[test]
    fn catalog_is_static_and_independent_of_config() {
        // Two calls return the same object; no file I/O is involved.
        let a = std::ptr::from_ref(scene_signal_catalog());
        let b = std::ptr::from_ref(scene_signal_catalog());
        assert_eq!(a, b);
    }
}
