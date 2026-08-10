//! Shared identifier mechanism: [`DomStr`] and [`domain_id!`] (ADR-030).
//!
//! Each consumer crate declares its own vocabulary instances. This crate owns
//! only the mechanism — never semantic ids.

use std::borrow::Borrow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

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

/// Ordered by name so identifiers can key a `BTreeMap` — control code needs
/// deterministic iteration order, not just lookup.
impl Ord for DomStr {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl PartialOrd for DomStr {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
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

/// Declares a domain identifier newtype backed by [`DomStr`].
#[macro_export]
macro_rules! domain_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name($crate::DomStr);

        impl $name {
            #[must_use]
            pub fn new(value: impl AsRef<str>) -> Self {
                Self($crate::DomStr::new(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl ::std::ops::Deref for $name {
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

        impl ::std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl ::std::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_tuple(stringify!($name))
                    .field(&self.as_str())
                    .finish()
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::collections::{HashMap, HashSet};

    domain_id!(TestId, "Identifier used only by this crate's tests.");
    domain_id!(OtherId, "A second identifier, to check the types stay distinct.");

    fn hash_of<T: Hash>(value: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    /// `Borrow<str>` is only sound if the borrowed form hashes identically.
    /// Break this and every `HashMap<Id, _>::get(&str)` silently misses —
    /// FSM states and zones would resolve to `None` instead of failing loudly.
    #[test]
    fn borrowed_lookup_hashes_like_the_owned_id() {
        assert_eq!(hash_of(&TestId::new("bed-approach")), hash_of(&"bed-approach"));
        assert_eq!(hash_of(&DomStr::new("bed-approach")), hash_of(&"bed-approach"));

        let mut map = HashMap::new();
        map.insert(TestId::new("in_bed"), 7u8);
        assert_eq!(map.get("in_bed"), Some(&7));
        assert_eq!(map.get("out_of_bed"), None);

        let set: HashSet<TestId> = ["a", "b"].into_iter().map(TestId::new).collect();
        assert!(set.contains("a"));
        assert!(!set.contains("c"));
    }

    #[test]
    fn equality_is_by_value_not_by_pointer() {
        let a = TestId::new("person");
        let b = TestId::new(String::from("person"));
        assert_eq!(a, b);
        assert_eq!(a, *"person");
        assert_eq!(a, "person");
        assert_ne!(a, TestId::new("face"));
    }

    #[test]
    fn clone_shares_the_backing_allocation() {
        let a = TestId::new("detect-fast");
        let b = a.clone();
        assert!(std::ptr::eq(a.as_str().as_ptr(), b.as_str().as_ptr()));
    }

    #[test]
    fn deref_and_display_expose_the_raw_name() {
        let id = TestId::new("blind");
        assert_eq!(id.as_str(), "blind");
        assert_eq!(&*id, "blind");
        assert_eq!(id.len(), 5);
        assert_eq!(id.to_string(), "blind");
        assert_eq!(format!("{id:?}"), r#"TestId("blind")"#);
    }

    /// Distinct vocabularies must not be interchangeable: this is the whole
    /// point of declaring ids per crate instead of passing `String` around.
    #[test]
    fn distinct_id_types_do_not_share_a_value_space() {
        let model = OtherId::new("person");
        let class = TestId::new("person");
        assert_eq!(model.as_str(), class.as_str());
        // `model == class` does not compile: no cross-type PartialEq exists.
        let mut map: HashMap<TestId, u8> = HashMap::new();
        map.insert(class, 1);
        assert_eq!(map.get(model.as_str()), Some(&1));
    }

    #[test]
    fn empty_and_unicode_names_round_trip() {
        assert_eq!(TestId::new("").as_str(), "");
        let id = TestId::new("zona-año");
        assert_eq!(id.as_str(), "zona-año");
        assert_eq!(hash_of(&id), hash_of(&"zona-año"));
    }
}
