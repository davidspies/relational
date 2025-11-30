//! Map operator - stateless, transforms each tuple.
//!
//! Implemented using flat_map.

use super::ops_flat_map::flat_map;
use super::relation::Op;

/// Create a map relation.
pub fn map<T, U, F, R>(input: R, f: F) -> impl Op<U>
where
    F: Fn(T) -> U,
    R: Op<T>,
{
    flat_map(input, move |t| std::iter::once(f(t)))
}
