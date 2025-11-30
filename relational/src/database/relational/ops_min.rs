//! Min operator - minimum value by key.
//!
//! Implemented as max with Reverse, then unwrapping.

use std::cmp::Reverse;
use std::hash::Hash;

use super::relation::{Op, Relation};

impl<R> Relation<R> {
    /// Minimum value by key.
    pub fn min<T, K, V, FK, FV>(self, key_fn: FK, val_fn: FV) -> Relation<impl Op<(K, V)>>
    where
        R: Op<T>,
        K: Clone + Eq + Hash,
        V: Clone + Ord,
        FK: Fn(&T) -> K,
        FV: Fn(&T) -> V,
    {
        self.max(key_fn, move |t| Reverse(val_fn(t)))
            .map(|(k, Reverse(v))| (k, v))
    }
}
