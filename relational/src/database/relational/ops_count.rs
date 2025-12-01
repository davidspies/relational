//! Count operator - count tuples by key.
//!
//! Implemented using sum with value 1.

use std::hash::Hash;

use super::relation::{Op, Relation};

impl<R> Relation<R> {
    /// Count tuples by key.
    /// Input must be (K, V) tuples where K is the key.
    /// Output is (K, count) pairs.
    pub fn group_count<K, V>(self) -> Relation<impl Op<(K, i64)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
    {
        self.map(|(k, _): (K, V)| (k, 1i64)).group_sum()
    }
}
