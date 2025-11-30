//! Filter operator - stateless, filters tuples by predicate.
//!
//! Implemented using flat_map.

use super::ops_flat_map::flat_map;
use super::relation::Op;

/// Create a filter relation.
pub fn filter<T, F, R>(input: R, pred: F) -> impl Op<T>
where
    F: Fn(&T) -> bool,
    R: Op<T>,
{
    flat_map(input, move |t| if pred(&t) { Some(t) } else { None })
}
