//! Shared identifier mechanism and control-owned vocabulary.
//!
//! `DomStr` and `domain_id!` are the shared mechanism (ADR-030). Each crate
//! declares its own id instances: this crate owns `StateId` and `ZoneId`.
//! Application-layer ids (`ModelId`, `ClassName`) stay in the binary.

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
        #[derive(Clone, PartialEq, Eq, Hash)]
        pub struct $name($crate::domain::DomStr);

        impl $name {
            #[must_use]
            pub fn new(value: impl AsRef<str>) -> Self {
                Self($crate::domain::DomStr::new(value))
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

domain_id!(StateId, "FSM state identifier from the catalog.");
domain_id!(ZoneId, "Spatial zone identifier from the catalog.");

impl StateId {
    /// Structural safe state used after panics / data loss.
    pub const BLIND: &'static str = "blind";
}
