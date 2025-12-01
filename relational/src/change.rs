//! Change tracking for differential dataflow.
//!
//! A `Diff` represents the multiplicity change for a tuple.

/// A difference/multiplicity value. Positive means insertions, negative means deletions.
pub type Diff = i64;
