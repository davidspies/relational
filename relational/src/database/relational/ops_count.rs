//! Count operator - count tuples by key.
//!
//! Implemented using sum with value 1.

use std::hash::Hash;

use super::relation::{Op, Relation};

impl<R> Relation<R> {
    /// Count tuples by key.
    pub fn count<T, K, FK>(self, key_fn: FK) -> Relation<impl Op<(K, i64)>>
    where
        R: Op<T>,
        K: Clone + Eq + Hash,
        FK: Fn(&T) -> K,
    {
        self.sum(key_fn, |_| 1i64)
    }
}
