//! Map operator - stateless, transforms each tuple.
//!
//! Implemented using flat_map.

use crate::Tuple;

use super::ops_flat_map::flat_map;
use super::relation::Relation;

/// Create a map relation.
pub fn map<T, U, F, R>(input: R, f: F) -> impl Relation<U>
where
    T: Tuple + 'static,
    U: Tuple + 'static,
    F: Fn(T) -> U + 'static,
    R: Relation<T>,
{
    flat_map(input, move |t| std::iter::once(f(t)))
}
