//! Typed signal values, ratio bounds, and operator compatibility.

/// Closed set of value kinds a signal may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalKind {
    Bool,
    Count,
    Ratio,
    Label,
}

/// Proportion in `[0.0, 1.0]`. Construction rejects non-finite and out-of-range values.
///
/// Does **not** implement `PartialEq` or `Eq`: exact float equality is not part of
/// the clinical contract. Compare only through ordered operators on [`SignalValue`].
#[derive(Debug, Clone, Copy)]
pub struct Ratio(f32);

/// Why a ratio could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatioError {
    /// Value was NaN or infinite.
    NonFinite,
    /// Value was finite but outside `[0.0, 1.0]`.
    OutOfRange,
}

impl Ratio {
    /// Builds a ratio if `value` is finite and in `[0.0, 1.0]`.
    pub fn new(value: f32) -> Result<Self, RatioError> {
        if !value.is_finite() {
            return Err(RatioError::NonFinite);
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(RatioError::OutOfRange);
        }
        Ok(Self(value))
    }

    /// The underlying finite value in `[0.0, 1.0]`.
    ///
    /// Reading a ratio does not enable exact equality through the signal
    /// comparison API; callers still only get the finite, bounded value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A typed signal value. No implicit coercions between variants.
#[derive(Debug, Clone)]
pub enum SignalValue {
    Bool(bool),
    Count(u64),
    Ratio(Ratio),
    Label(String),
}

impl SignalValue {
    /// Kind of this value.
    #[must_use]
    pub const fn kind(&self) -> SignalKind {
        match self {
            Self::Bool(_) => SignalKind::Bool,
            Self::Count(_) => SignalKind::Count,
            Self::Ratio(_) => SignalKind::Ratio,
            Self::Label(_) => SignalKind::Label,
        }
    }

    /// Applies `op` between `self` (actual) and `expected`.
    ///
    /// Returns an error when kinds differ or the operator is incompatible with
    /// the kind. Never coerces types.
    pub fn compare(&self, op: SignalOp, expected: &Self) -> Result<bool, CompareError> {
        if self.kind() != expected.kind() {
            return Err(CompareError::KindMismatch {
                left: self.kind(),
                right: expected.kind(),
            });
        }
        if !op.is_compatible(self.kind()) {
            return Err(CompareError::IncompatibleOp {
                kind: self.kind(),
                op,
            });
        }

        let result = match (self, expected) {
            (Self::Bool(a), Self::Bool(b)) => match op {
                SignalOp::Eq => a == b,
                SignalOp::Ne => a != b,
                _ => unreachable!("op compatibility checked above"),
            },
            (Self::Count(a), Self::Count(b)) => match op {
                SignalOp::Eq => a == b,
                SignalOp::Ne => a != b,
                SignalOp::Gte => a >= b,
                SignalOp::Lte => a <= b,
                SignalOp::Gt => a > b,
                SignalOp::Lt => a < b,
            },
            (Self::Ratio(a), Self::Ratio(b)) => match op {
                SignalOp::Gte => a.get() >= b.get(),
                SignalOp::Lte => a.get() <= b.get(),
                SignalOp::Gt => a.get() > b.get(),
                SignalOp::Lt => a.get() < b.get(),
                SignalOp::Eq | SignalOp::Ne => {
                    unreachable!("ratio equality rejected by compatibility")
                }
            },
            (Self::Label(a), Self::Label(b)) => match op {
                SignalOp::Eq => a == b,
                SignalOp::Ne => a != b,
                _ => unreachable!("op compatibility checked above"),
            },
            _ => unreachable!("kind mismatch rejected above"),
        };
        Ok(result)
    }
}

/// Comparison operator for a signal guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalOp {
    Eq,
    Ne,
    Gte,
    Lte,
    Gt,
    Lt,
}

impl SignalOp {
    /// Parses the symbolic operator used by FSM TOML.
    #[must_use]
    pub fn parse(symbol: &str) -> Option<Self> {
        match symbol {
            "==" => Some(Self::Eq),
            "!=" => Some(Self::Ne),
            ">=" => Some(Self::Gte),
            "<=" => Some(Self::Lte),
            ">" => Some(Self::Gt),
            "<" => Some(Self::Lt),
            _ => None,
        }
    }

    /// Symbol used by the configuration format and diagnostics.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Gte => ">=",
            Self::Lte => "<=",
            Self::Gt => ">",
            Self::Lt => "<",
        }
    }

    /// Whether this operator may be applied to values of `kind`.
    #[must_use]
    pub const fn is_compatible(self, kind: SignalKind) -> bool {
        match kind {
            SignalKind::Bool | SignalKind::Label => matches!(self, Self::Eq | Self::Ne),
            SignalKind::Count => true,
            SignalKind::Ratio => matches!(self, Self::Gte | Self::Lte | Self::Gt | Self::Lt),
        }
    }

    /// Checks compatibility and returns a structured error when it fails.
    pub fn require_compatible(self, kind: SignalKind) -> Result<(), OpCompatibilityError> {
        if self.is_compatible(kind) {
            Ok(())
        } else {
            Err(OpCompatibilityError { kind, op: self })
        }
    }
}

/// Operator is not valid for the given kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpCompatibilityError {
    pub kind: SignalKind,
    pub op: SignalOp,
}

/// Failure comparing two signal values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareError {
    KindMismatch { left: SignalKind, right: SignalKind },
    IncompatibleOp { kind: SignalKind, op: SignalOp },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_accepts_bounds() {
        assert!(Ratio::new(0.0).is_ok());
        assert!(Ratio::new(1.0).is_ok());
        assert!(Ratio::new(0.5).is_ok());
        // Exact 0.0/1.0 round-trip is part of the construction contract.
        assert!((Ratio::new(0.0).unwrap().get() - 0.0).abs() < f32::EPSILON);
        assert!((Ratio::new(1.0).unwrap().get() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ratio_rejects_out_of_range() {
        assert_eq!(Ratio::new(-0.01).unwrap_err(), RatioError::OutOfRange);
        assert_eq!(Ratio::new(1.01).unwrap_err(), RatioError::OutOfRange);
        assert_eq!(Ratio::new(-1.0).unwrap_err(), RatioError::OutOfRange);
        assert_eq!(Ratio::new(2.0).unwrap_err(), RatioError::OutOfRange);
    }

    #[test]
    fn ratio_rejects_non_finite() {
        assert_eq!(Ratio::new(f32::NAN).unwrap_err(), RatioError::NonFinite);
        assert_eq!(
            Ratio::new(f32::INFINITY).unwrap_err(),
            RatioError::NonFinite
        );
        assert_eq!(
            Ratio::new(f32::NEG_INFINITY).unwrap_err(),
            RatioError::NonFinite
        );
    }

    #[test]
    fn operator_matrix_bool_and_label() {
        for op in [SignalOp::Eq, SignalOp::Ne] {
            assert!(op.is_compatible(SignalKind::Bool));
            assert!(op.is_compatible(SignalKind::Label));
        }
        for op in [SignalOp::Gte, SignalOp::Lte, SignalOp::Gt, SignalOp::Lt] {
            assert!(!op.is_compatible(SignalKind::Bool));
            assert!(!op.is_compatible(SignalKind::Label));
            assert!(op.require_compatible(SignalKind::Bool).is_err());
        }
    }

    #[test]
    fn operator_matrix_count_allows_all() {
        for op in [
            SignalOp::Eq,
            SignalOp::Ne,
            SignalOp::Gte,
            SignalOp::Lte,
            SignalOp::Gt,
            SignalOp::Lt,
        ] {
            assert!(op.is_compatible(SignalKind::Count));
        }
    }

    #[test]
    fn operator_matrix_ratio_rejects_equality() {
        for op in [SignalOp::Gte, SignalOp::Lte, SignalOp::Gt, SignalOp::Lt] {
            assert!(op.is_compatible(SignalKind::Ratio));
        }
        assert!(!SignalOp::Eq.is_compatible(SignalKind::Ratio));
        assert!(!SignalOp::Ne.is_compatible(SignalKind::Ratio));
        assert!(SignalOp::Eq.require_compatible(SignalKind::Ratio).is_err());
        assert!(SignalOp::Ne.require_compatible(SignalKind::Ratio).is_err());
    }

    #[test]
    fn compare_bool_eq_ne() {
        let t = SignalValue::Bool(true);
        let f = SignalValue::Bool(false);
        assert!(t.compare(SignalOp::Eq, &t).unwrap());
        assert!(!t.compare(SignalOp::Eq, &f).unwrap());
        assert!(t.compare(SignalOp::Ne, &f).unwrap());
        assert!(t.compare(SignalOp::Gte, &t).is_err());
    }

    #[test]
    fn compare_count_ordered_ops() {
        let a = SignalValue::Count(3);
        let b = SignalValue::Count(5);
        assert!(a.compare(SignalOp::Lt, &b).unwrap());
        assert!(a.compare(SignalOp::Lte, &b).unwrap());
        assert!(!a.compare(SignalOp::Gt, &b).unwrap());
        assert!(a.compare(SignalOp::Gte, &a).unwrap());
        assert!(a.compare(SignalOp::Eq, &a).unwrap());
        assert!(a.compare(SignalOp::Ne, &b).unwrap());
    }

    #[test]
    fn compare_ratio_ordered_only() {
        let low = SignalValue::Ratio(Ratio::new(0.3).unwrap());
        let high = SignalValue::Ratio(Ratio::new(0.8).unwrap());
        assert!(low.compare(SignalOp::Lt, &high).unwrap());
        assert!(low.compare(SignalOp::Lte, &high).unwrap());
        assert!(high.compare(SignalOp::Gte, &low).unwrap());
        assert!(high.compare(SignalOp::Gt, &low).unwrap());
        assert!(matches!(
            low.compare(SignalOp::Eq, &high),
            Err(CompareError::IncompatibleOp {
                kind: SignalKind::Ratio,
                op: SignalOp::Eq
            })
        ));
        assert!(matches!(
            low.compare(SignalOp::Ne, &high),
            Err(CompareError::IncompatibleOp {
                kind: SignalKind::Ratio,
                op: SignalOp::Ne
            })
        ));
    }

    #[test]
    fn compare_label_eq_ne() {
        let a = SignalValue::Label("single".into());
        let b = SignalValue::Label("multiple".into());
        assert!(a.compare(SignalOp::Eq, &a).unwrap());
        assert!(a.compare(SignalOp::Ne, &b).unwrap());
        assert!(a.compare(SignalOp::Lt, &b).is_err());
    }

    #[test]
    fn compare_rejects_kind_mismatch_without_coercion() {
        let b = SignalValue::Bool(true);
        let c = SignalValue::Count(1);
        let r = SignalValue::Ratio(Ratio::new(0.5).unwrap());
        let l = SignalValue::Label("single".into());
        assert!(matches!(
            b.compare(SignalOp::Eq, &c),
            Err(CompareError::KindMismatch { .. })
        ));
        assert!(matches!(
            c.compare(SignalOp::Gte, &r),
            Err(CompareError::KindMismatch { .. })
        ));
        assert!(matches!(
            l.compare(SignalOp::Eq, &b),
            Err(CompareError::KindMismatch { .. })
        ));
    }

    #[test]
    fn signal_value_kind_matches_variant() {
        assert_eq!(SignalValue::Bool(false).kind(), SignalKind::Bool);
        assert_eq!(SignalValue::Count(0).kind(), SignalKind::Count);
        assert_eq!(
            SignalValue::Ratio(Ratio::new(0.0).unwrap()).kind(),
            SignalKind::Ratio
        );
        assert_eq!(SignalValue::Label("empty".into()).kind(), SignalKind::Label);
    }
}
