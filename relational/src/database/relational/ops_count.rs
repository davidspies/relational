//! Count operator - count tuples by key.
//!
//! Implemented using sum with value 1.

use std::hash::Hash;

use super::ops_sum::sum;
use super::relation::{Op, Relation};

/// Create a count relation - counts tuples by key.
pub fn count<T, K: Clone + Eq + Hash, FK, R>(
    input: Relation<R>,
    key_fn: FK,
) -> Relation<impl Op<(K, i64)>>
where
    FK: Fn(&T) -> K,
    R: Op<T>,
{
    sum(input, key_fn, |_| 1i64)
}
