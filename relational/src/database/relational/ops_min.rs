//! Min operator - minimum value by key.
//!
//! Implemented as max with Reverse, then unwrapping.

use std::cmp::Reverse;
use std::hash::Hash;

use super::relation::{Op, Relation};

impl<R> Relation<R> {
    /// Minimum value by key.
    /// Input must be (K, V) tuples where K is the key and V is the value.
    pub fn min<K, V>(self) -> Relation<impl Op<(K, V)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Ord,
    {
        self.map(|(k, v)| (k, Reverse(v)))
            .max()
            .map(|(k, Reverse(v))| (k, v))
    }
}
