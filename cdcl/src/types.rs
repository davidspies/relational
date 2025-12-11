//! Types for CDCL SAT solver: Lit, Var, Level, ClauseId, Conflict.

use std::{fmt, ops::Not};

/// A literal is a variable with a sign (positive or negative).
/// Positive values represent the variable, negative values represent its negation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Lit(i32);

impl Lit {
    /// Create a positive literal for a variable.
    pub fn pos(v: Var) -> Self {
        Lit(v.0 as i32)
    }

    /// Create a negative literal for a variable.
    pub fn neg(v: Var) -> Self {
        Lit(-(v.0 as i32))
    }

    /// Create a literal from a raw i32 (positive = positive literal, negative = negative literal).
    pub fn from_raw(raw: i32) -> Self {
        assert!(raw != 0, "Literal cannot be 0");
        Lit(raw)
    }

    /// Get the variable this literal refers to.
    pub fn var(self) -> Var {
        Var(self.0.unsigned_abs())
    }

    /// Check if this is a positive literal.
    pub fn is_positive(self) -> bool {
        self.0 > 0
    }

    /// Get the raw i32 value.
    pub fn raw(self) -> i32 {
        self.0
    }
}

impl Not for Lit {
    type Output = Lit;

    fn not(self) -> Self::Output {
        Lit(-self.0)
    }
}

impl fmt::Display for Lit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 > 0 {
            write!(f, "x{}", self.0)
        } else {
            write!(f, "¬x{}", -self.0)
        }
    }
}

/// A variable identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Var(pub(super) u32);

impl Var {
    /// Create a variable from a 1-indexed number.
    pub fn new(n: u32) -> Self {
        assert!(n > 0, "Variables are 1-indexed");
        Var(n)
    }

    /// Get the raw u32 value.
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for Var {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "x{}", self.0)
    }
}

/// Decision level (0 = top-level/forced, 1+ = decision levels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Level(u32);

impl Level {
    /// The top level (level 0) where unit clauses propagate.
    pub const TOP: Level = Level(0);

    /// Create a new level.
    #[cfg(test)]
    pub(crate) fn new(n: u32) -> Self {
        Level(n)
    }

    /// Get the raw u32 value.
    pub(crate) fn raw(self) -> u32 {
        self.0
    }

    /// Increment the level.
    pub(crate) fn inc(&mut self) {
        self.0 += 1;
    }

    /// Decrement the level.
    pub(crate) fn dec(&mut self) {
        self.0 = self.0.saturating_sub(1);
    }
}

/// Weight in a PB constraint (i64 for compatibility with group_sum).
pub type Weight = i64;

/// A constraint ID distinguishing original constraints from learned ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum ConstraintId {
    /// An original constraint from the input formula.
    Original(u32),
    /// A learned constraint from conflict analysis.
    Learned(u32),
}

/// The cause of a literal assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Cause {
    /// The literal was assigned without a constraint (decision or external).
    NoConstraint,
    /// The literal was propagated from a constraint.
    FromConstraint(ConstraintId),
}

/// A conflict detected during propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Conflict {
    /// A constraint is violated (slack < 0).
    UnsatConstraint(ConstraintId),
    /// Both a literal and its negation are assigned.
    DirectConflict(Var),
}
