//! Types for CDCL SAT solver: Lit, Var, Level, ClauseId, Conflict.

use std::fmt;

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

    /// Get the negation of this literal.
    pub fn negated(self) -> Self {
        Lit(-self.0)
    }

    /// Get the raw i32 value.
    pub fn raw(self) -> i32 {
        self.0
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
    pub fn new(n: u32) -> Self {
        Level(n)
    }

    /// Get the raw u32 value.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Increment the level.
    pub fn inc(&mut self) {
        self.0 += 1;
    }

    /// Decrement the level.
    pub fn dec(&mut self) {
        self.0 = self.0.saturating_sub(1);
    }
}

/// A clause ID for tracking which clause caused an implication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ClauseId(u32);

impl ClauseId {
    /// A special clause ID indicating a decision (no reason clause).
    pub const DECISION: ClauseId = ClauseId(0);

    /// Create a new clause ID.
    pub fn new(n: u32) -> Self {
        ClauseId(n)
    }

    /// Get the raw u32 value.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Check if this is a decision (no reason clause).
    pub fn is_decision(self) -> bool {
        self.0 == 0
    }
}

/// A conflict detected during propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Conflict {
    /// A clause has all its literals assigned false.
    EmptyClause(ClauseId),
    /// Both a literal and its negation are assigned.
    /// The variable is stored (the literal that was assigned both ways).
    DirectConflict(Var),
}

/// Helper function to get the variable of a literal.
pub(super) fn var(lit: Lit) -> Var {
    lit.var()
}
