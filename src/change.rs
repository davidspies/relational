//! Change tracking for differential dataflow.
//!
//! A `Diff` represents the multiplicity change for a tuple.
//! A `Change` pairs a tuple with its diff.

use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// A difference/multiplicity value. Positive means insertions, negative means deletions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Diff(pub i64);

impl Diff {
    pub const ZERO: Diff = Diff(0);
    pub const ONE: Diff = Diff(1);
    pub const NEG_ONE: Diff = Diff(-1);

    #[inline]
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn is_positive(self) -> bool {
        self.0 > 0
    }

    #[inline]
    pub fn is_negative(self) -> bool {
        self.0 < 0
    }
}

impl Add for Diff {
    type Output = Diff;
    #[inline]
    fn add(self, rhs: Diff) -> Diff {
        Diff(self.0 + rhs.0)
    }
}

impl AddAssign for Diff {
    #[inline]
    fn add_assign(&mut self, rhs: Diff) {
        self.0 += rhs.0;
    }
}

impl Sub for Diff {
    type Output = Diff;
    #[inline]
    fn sub(self, rhs: Diff) -> Diff {
        Diff(self.0 - rhs.0)
    }
}

impl SubAssign for Diff {
    #[inline]
    fn sub_assign(&mut self, rhs: Diff) {
        self.0 -= rhs.0;
    }
}

impl Neg for Diff {
    type Output = Diff;
    #[inline]
    fn neg(self) -> Diff {
        Diff(-self.0)
    }
}

impl std::ops::Mul<i64> for Diff {
    type Output = Diff;
    #[inline]
    fn mul(self, rhs: i64) -> Diff {
        Diff(self.0 * rhs)
    }
}

impl From<i64> for Diff {
    fn from(v: i64) -> Self {
        Diff(v)
    }
}

impl From<Diff> for i64 {
    fn from(d: Diff) -> Self {
        d.0
    }
}

/// A change to a tuple: the tuple value paired with its diff.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Change<T> {
    pub tuple: T,
    pub diff: Diff,
}

impl<T> Change<T> {
    pub fn new(tuple: T, diff: Diff) -> Self {
        Change { tuple, diff }
    }

    pub fn insert(tuple: T) -> Self {
        Change::new(tuple, Diff::ONE)
    }

    pub fn delete(tuple: T) -> Self {
        Change::new(tuple, Diff::NEG_ONE)
    }

    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Change<U> {
        Change {
            tuple: f(self.tuple),
            diff: self.diff,
        }
    }
}

impl<T: Clone> Change<T> {
    pub fn negate(&self) -> Self {
        Change {
            tuple: self.tuple.clone(),
            diff: -self.diff,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_arithmetic() {
        assert_eq!(Diff(3) + Diff(2), Diff(5));
        assert_eq!(Diff(3) - Diff(2), Diff(1));
        assert_eq!(-Diff(3), Diff(-3));
        assert_eq!(Diff(3) * 2, Diff(6));
    }

    #[test]
    fn test_change_creation() {
        let insert = Change::insert(42);
        assert_eq!(insert.tuple, 42);
        assert_eq!(insert.diff, Diff::ONE);

        let delete = Change::delete(42);
        assert_eq!(delete.tuple, 42);
        assert_eq!(delete.diff, Diff::NEG_ONE);
    }
}
