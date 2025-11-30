//! Count operator - count tuples by key.
//!
//! Implemented using sum with value 1.

use std::hash::Hash;

use crate::Tuple;

use super::ops_sum::sum;
use super::relation::Relation;

/// Create a count relation - counts tuples by key.
pub fn count<T, K, FK, R>(input: R, key_fn: FK) -> impl Relation<(K, i64)>
where
    T: Tuple + 'static,
    K: Tuple + Eq + Hash + 'static,
    FK: Fn(&T) -> K + 'static,
    R: Relation<T>,
{
    sum(input, key_fn, |_| 1i64)
}
