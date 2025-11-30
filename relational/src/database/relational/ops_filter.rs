//! Filter operator - stateless, filters tuples by predicate.
//!
//! Implemented using flat_map.

use super::relation::{Op, Relation};

impl<R> Relation<R> {
    /// Filter tuples by predicate.
    pub fn filter<T, F>(self, pred: F) -> Relation<impl Op<T>>
    where
        R: Op<T>,
        F: Fn(&T) -> bool,
    {
        self.flat_map(move |t| if pred(&t) { Some(t) } else { None })
    }
}
