//! Per-cycle signal table and immutable snapshot.

use std::collections::BTreeMap;

use crate::domain::SignalTag;

use super::{SignalCatalog, SignalKind, SignalOp, SignalValue, value::CompareError};

/// Why an insertion into the table was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalTableError {
    /// Tag is not present in the catalog (wrapping a string is not enough).
    UnknownTag { tag: SignalTag },
    /// Value kind does not match the catalog descriptor.
    KindMismatch {
        tag: SignalTag,
        expected: SignalKind,
        actual: SignalKind,
    },
    /// Label is not in the closed set for this tag.
    InvalidLabel { tag: SignalTag, label: String },
}

/// Mutable builder of one control-cycle signal set.
///
/// A fresh table starts empty. Absence is the lack of an entry — never a
/// default value. There is no `clear()`: producers build a new table each tick.
#[derive(Debug, Clone, Default)]
pub struct SignalTable {
    values: BTreeMap<SignalTag, SignalValue>,
}

impl SignalTable {
    /// Empty table for a new control cycle.
    #[must_use]
    pub fn new() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// Inserts `value` for `tag` after validating against `catalog`.
    ///
    /// Does not normalize, clamp, or coerce. A failure is a producer/catalog
    /// defect (later stages map this to `SignalFault`).
    pub fn insert(
        &mut self,
        catalog: &SignalCatalog,
        tag: SignalTag,
        value: SignalValue,
    ) -> Result<(), SignalTableError> {
        let Some(desc) = catalog.get(&tag) else {
            return Err(SignalTableError::UnknownTag { tag });
        };

        let actual = value.kind();
        if actual != desc.kind() {
            return Err(SignalTableError::KindMismatch {
                tag,
                expected: desc.kind(),
                actual,
            });
        }

        if let SignalValue::Label(ref label) = value
            && !desc.allows_label(label)
        {
            return Err(SignalTableError::InvalidLabel {
                tag,
                label: label.clone(),
            });
        }

        self.values.insert(tag, value);
        Ok(())
    }

    /// Value for `tag`, if present this cycle.
    ///
    /// `None` means absence and is distinct from `Some(SignalValue::Bool(false))`.
    #[must_use]
    pub fn get(&self, tag: &SignalTag) -> Option<&SignalValue> {
        self.values.get(tag)
    }

    /// Whether the tag has a present value this cycle.
    #[must_use]
    pub fn contains(&self, tag: &SignalTag) -> bool {
        self.values.contains_key(tag)
    }

    /// Number of present (non-absent) signals.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Deterministic iteration over present entries (tag order).
    pub fn iter(&self) -> impl Iterator<Item = (&SignalTag, &SignalValue)> {
        self.values.iter()
    }

    /// Evaluates `(tag, op, expected)` against the present value.
    ///
    /// An absent tag never matches — including against `Ne` — so optional
    /// signals such as `cara.en_dwell` stay distinct from `Bool(false)`.
    ///
    /// Invalid type/operator combinations are returned as errors rather than
    /// being mistaken for a normal non-match.
    pub fn matches(
        &self,
        tag: &SignalTag,
        op: SignalOp,
        expected: &SignalValue,
    ) -> Result<bool, CompareError> {
        let Some(actual) = self.values.get(tag) else {
            return Ok(false);
        };
        actual.compare(op, expected)
    }

    /// Frozen, ordered view of this table for the cycle.
    ///
    /// Includes every catalog tag: present values and explicit absences.
    /// No JSON or `SceneEvent` wiring in stage A.
    #[must_use]
    pub fn snapshot(&self, catalog: &SignalCatalog) -> SceneSignalsSnapshot {
        let mut entries = BTreeMap::new();
        for (tag, _) in catalog.iter() {
            entries.insert(tag.clone(), self.values.get(tag).cloned());
        }
        SceneSignalsSnapshot {
            catalog_version: catalog.version(),
            entries,
        }
    }
}

/// Immutable, ordered view of all catalog tags for one cycle.
#[derive(Debug, Clone)]
pub struct SceneSignalsSnapshot {
    catalog_version: u32,
    entries: BTreeMap<SignalTag, Option<SignalValue>>,
}

impl SceneSignalsSnapshot {
    #[must_use]
    pub fn catalog_version(&self) -> u32 {
        self.catalog_version
    }

    /// Value for `tag` in this snapshot (`None` = absent).
    #[must_use]
    pub fn get(&self, tag: &SignalTag) -> Option<&SignalValue> {
        self.entries.get(tag).and_then(|v| v.as_ref())
    }

    /// Whether the snapshot knows about `tag` (declared in the catalog used
    /// to build it), regardless of presence.
    #[must_use]
    pub fn declares(&self, tag: &SignalTag) -> bool {
        self.entries.contains_key(tag)
    }

    /// Whether `tag` is declared and has no value this cycle.
    #[must_use]
    pub fn is_absent(&self, tag: &SignalTag) -> bool {
        matches!(self.entries.get(tag), Some(None))
    }

    /// Stable iteration: `(tag, Option<&SignalValue>)` in tag order.
    pub fn iter(&self) -> impl Iterator<Item = (&SignalTag, Option<&SignalValue>)> {
        self.entries
            .iter()
            .map(|(tag, value)| (tag, value.as_ref()))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for SceneSignalsSnapshot {
    fn default() -> Self {
        SignalTable::new().snapshot(super::catalog::scene_signal_catalog())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::{Ratio, scene_signal_catalog};

    fn tag(name: &str) -> SignalTag {
        SignalTag::new(name)
    }

    #[test]
    fn new_table_is_empty_and_has_no_inherited_values() {
        let cat = scene_signal_catalog();
        let mut first = SignalTable::new();
        first
            .insert(cat, tag("persona.presente"), SignalValue::Bool(true))
            .unwrap();
        assert_eq!(first.len(), 1);

        let second = SignalTable::new();
        assert!(second.is_empty());
        assert!(second.get(&tag("persona.presente")).is_none());
    }

    #[test]
    fn insert_and_read_round_trip() {
        let cat = scene_signal_catalog();
        let mut table = SignalTable::new();
        table
            .insert(cat, tag("persona.cantidad"), SignalValue::Count(2))
            .unwrap();
        table
            .insert(
                cat,
                tag("ocupacion.cardinalidad"),
                SignalValue::Label("multiple".into()),
            )
            .unwrap();

        match table.get(&tag("persona.cantidad")) {
            Some(SignalValue::Count(2)) => {}
            other => panic!("unexpected {other:?}"),
        }
        match table.get(&tag("ocupacion.cardinalidad")) {
            Some(SignalValue::Label(s)) if s == "multiple" => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn absence_is_distinct_from_bool_false() {
        let cat = scene_signal_catalog();
        let mut table = SignalTable::new();

        // No ROI case: tag never inserted.
        assert!(table.get(&tag("cara.en_dwell")).is_none());
        assert!(!table.contains(&tag("cara.en_dwell")));
        assert!(
            !table
                .matches(
                    &tag("cara.en_dwell"),
                    SignalOp::Eq,
                    &SignalValue::Bool(false)
                )
                .unwrap()
        );
        assert!(
            !table
                .matches(
                    &tag("cara.en_dwell"),
                    SignalOp::Ne,
                    &SignalValue::Bool(true)
                )
                .unwrap()
        );

        // ROI present, condition negative.
        table
            .insert(cat, tag("cara.en_dwell"), SignalValue::Bool(false))
            .unwrap();
        match table.get(&tag("cara.en_dwell")) {
            Some(SignalValue::Bool(false)) => {}
            other => panic!("expected Some(false), got {other:?}"),
        }
        assert!(
            table
                .matches(
                    &tag("cara.en_dwell"),
                    SignalOp::Eq,
                    &SignalValue::Bool(false)
                )
                .unwrap()
        );
    }

    #[test]
    fn reject_unknown_tag() {
        let cat = scene_signal_catalog();
        let mut table = SignalTable::new();
        let err = table
            .insert(cat, tag("zona.ocupada"), SignalValue::Bool(true))
            .unwrap_err();
        assert!(matches!(err, SignalTableError::UnknownTag { .. }));
    }

    #[test]
    fn reject_kind_mismatch() {
        let cat = scene_signal_catalog();
        let mut table = SignalTable::new();
        let err = table
            .insert(cat, tag("persona.presente"), SignalValue::Count(1))
            .unwrap_err();
        assert!(matches!(
            err,
            SignalTableError::KindMismatch {
                expected: SignalKind::Bool,
                actual: SignalKind::Count,
                ..
            }
        ));
    }

    #[test]
    fn reject_invalid_label() {
        let cat = scene_signal_catalog();
        let mut table = SignalTable::new();
        let err = table
            .insert(
                cat,
                tag("ocupacion.cardinalidad"),
                SignalValue::Label("full".into()),
            )
            .unwrap_err();
        assert!(matches!(
            err,
            SignalTableError::InvalidLabel { label, .. } if label == "full"
        ));
    }

    #[test]
    fn iteration_order_is_deterministic() {
        let cat = scene_signal_catalog();
        let mut table = SignalTable::new();
        // Insert out of order.
        table
            .insert(cat, tag("persona.cantidad"), SignalValue::Count(1))
            .unwrap();
        table
            .insert(cat, tag("cara.presente"), SignalValue::Bool(true))
            .unwrap();
        table
            .insert(cat, tag("persona.presente"), SignalValue::Bool(true))
            .unwrap();

        let names: Vec<&str> = table.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            names,
            vec!["cara.presente", "persona.cantidad", "persona.presente"]
        );

        let mut again = SignalTable::new();
        again
            .insert(cat, tag("persona.presente"), SignalValue::Bool(true))
            .unwrap();
        again
            .insert(cat, tag("cara.presente"), SignalValue::Bool(true))
            .unwrap();
        again
            .insert(cat, tag("persona.cantidad"), SignalValue::Count(1))
            .unwrap();
        let names2: Vec<&str> = again.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(names, names2);
    }

    #[test]
    fn snapshot_includes_all_catalog_tags_with_absences() {
        let cat = scene_signal_catalog();
        let mut table = SignalTable::new();
        table
            .insert(cat, tag("persona.presente"), SignalValue::Bool(true))
            .unwrap();
        table
            .insert(
                cat,
                tag("cara.confianza"),
                SignalValue::Ratio(Ratio::new(0.9).unwrap()),
            )
            .unwrap();

        let snap = table.snapshot(cat);
        assert_eq!(snap.catalog_version(), 1);
        assert_eq!(snap.len(), 9);

        match snap.get(&tag("persona.presente")) {
            Some(SignalValue::Bool(true)) => {}
            other => panic!("unexpected {other:?}"),
        }
        assert!(snap.is_absent(&tag("cara.en_dwell")));
        assert!(snap.declares(&tag("cara.en_dwell")));
        assert!(snap.get(&tag("cara.en_dwell")).is_none());

        let ordered: Vec<&str> = snap.iter().map(|(t, _)| t.as_str()).collect();
        let mut sorted = ordered.clone();
        sorted.sort_unstable();
        assert_eq!(ordered, sorted);
        assert_eq!(ordered.len(), 9);
    }

    #[test]
    fn ratio_insert_and_ordered_match() {
        let cat = scene_signal_catalog();
        let mut table = SignalTable::new();
        table
            .insert(
                cat,
                tag("cara.confianza"),
                SignalValue::Ratio(Ratio::new(0.87).unwrap()),
            )
            .unwrap();
        let threshold = SignalValue::Ratio(Ratio::new(0.80).unwrap());
        assert!(
            table
                .matches(&tag("cara.confianza"), SignalOp::Gte, &threshold)
                .unwrap()
        );
        assert!(
            !table
                .matches(&tag("cara.confianza"), SignalOp::Lt, &threshold)
                .unwrap()
        );
    }

    #[test]
    fn matches_surfaces_invalid_operator_and_type() {
        let cat = scene_signal_catalog();
        let mut table = SignalTable::new();
        table
            .insert(cat, tag("persona.presente"), SignalValue::Bool(true))
            .unwrap();

        assert!(matches!(
            table.matches(
                &tag("persona.presente"),
                SignalOp::Gte,
                &SignalValue::Bool(true),
            ),
            Err(CompareError::IncompatibleOp {
                kind: SignalKind::Bool,
                op: SignalOp::Gte,
            })
        ));
        assert!(matches!(
            table.matches(
                &tag("persona.presente"),
                SignalOp::Eq,
                &SignalValue::Count(1),
            ),
            Err(CompareError::KindMismatch {
                left: SignalKind::Bool,
                right: SignalKind::Count,
            })
        ));
    }

    #[test]
    fn snapshot_does_not_require_fsm_or_logger() {
        // Constructing a snapshot only needs catalog + table — stage A surface.
        let cat = scene_signal_catalog();
        let table = SignalTable::new();
        let snap = table.snapshot(cat);
        assert_eq!(snap.len(), cat.len());
    }
}
