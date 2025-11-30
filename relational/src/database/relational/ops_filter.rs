//! Filter operator - stateless, filters tuples by predicate.
//!
//! Implemented using flat_map.

use crate::Tuple;

use super::ops_flat_map::flat_map;
use super::relation::Relation;

/// Create a filter relation.
pub fn filter<T, F, R>(input: R, pred: F) -> impl Relation<T>
where
    T: Tuple + 'static,
    F: Fn(&T) -> bool + 'static,
    R: Relation<T>,
{
    flat_map(input, move |t| if pred(&t) { Some(t) } else { None })
}
