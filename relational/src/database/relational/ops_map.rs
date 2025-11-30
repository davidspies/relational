//! Map operator - stateless, transforms each tuple.
//!
//! Implemented using flat_map.

use super::relation::{Op, Relation};

impl<R> Relation<R> {
    /// Transform each tuple.
    pub fn map<T, U, F>(self, f: F) -> Relation<impl Op<U>>
    where
        R: Op<T>,
        F: Fn(T) -> U,
    {
        self.flat_map(move |t| std::iter::once(f(t)))
    }
}
