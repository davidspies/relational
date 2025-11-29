//! Min operator - minimum value by key.
//!
//! Implemented as max with Reverse, then unwrapping.

use std::cmp::Reverse;
use std::hash::Hash;

use crate::Tuple;

use super::ops_map::map;
use super::ops_max::max;
use super::relation::Relation;

/// Create a min relation - minimum value by key.
pub fn min<T, K, V, FK, FV, R>(
    input: R,
    key_fn: FK,
    val_fn: FV,
) -> impl Relation<(K, V)>
where
    T: Tuple + 'static,
    K: Tuple + Eq + Hash + Clone + 'static,
    V: Tuple + Ord + Clone + 'static,
    FK: Fn(&T) -> K + 'static,
    FV: Fn(&T) -> V + 'static,
    R: Relation<T>,
{
    let with_reverse = max(input, key_fn, move |t| Reverse(val_fn(t)));
    map(with_reverse, |(k, Reverse(v))| (k, v))
}
