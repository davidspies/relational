//! Min operator - minimum value by key.
//!
//! Implemented as max with Reverse, then unwrapping.

use std::cmp::Reverse;
use std::hash::Hash;

use super::ops_map::map;
use super::ops_max::max;
use super::relation::{Op, Relation};

/// Create a min relation - minimum value by key.
pub fn min<T, K, V, FK, FV, R>(
    input: Relation<R>,
    key_fn: FK,
    val_fn: FV,
) -> Relation<impl Op<(K, V)>>
where
    K: Clone + Eq + Hash,
    V: Clone + Ord,
    FK: Fn(&T) -> K,
    FV: Fn(&T) -> V,
    R: Op<T>,
{
    let with_reverse = max(input, key_fn, move |t| Reverse(val_fn(t)));
    map(with_reverse, |(k, Reverse(v))| (k, v))
}
