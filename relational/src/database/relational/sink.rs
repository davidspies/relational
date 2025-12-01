//! Sink trait for receiving relation changes.

use std::hash::Hash;

use crate::change::Diff;
use crate::collection::Multiset;

/// A sink that can receive changes from a relation.
///
/// Implement this trait to create custom data structures that accumulate
/// relation changes in different ways.
pub trait Sink<T> {
    /// Apply a single change (tuple with diff) to the sink.
    fn apply(&mut self, tuple: T, diff: Diff);
}

impl<T: Eq + Hash> Sink<T> for Multiset<T> {
    fn apply(&mut self, tuple: T, diff: Diff) {
        self.update(tuple, diff);
    }
}
