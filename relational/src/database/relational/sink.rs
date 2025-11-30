//! Sink trait for receiving relation changes.

use crate::Tuple;
use crate::change::{Change, Diff};
use crate::collection::Multiset;

/// A sink that can receive changes from a relation.
///
/// Implement this trait to create custom data structures that accumulate
/// relation changes in different ways.
pub trait Sink<T: Tuple> {
    /// Apply a single change (tuple with diff) to the sink.
    fn apply(&mut self, tuple: T, diff: Diff);
}

impl<T: Tuple> Sink<T> for Multiset<T> {
    fn apply(&mut self, tuple: T, diff: Diff) {
        self.apply_change(Change::new(tuple, diff));
    }
}
