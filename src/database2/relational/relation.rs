//! Relation - a differential stream of changes.
//!
//! Relations are move-only (not Clone). To use a relation in multiple places,
//! you must first save it to get a `SavedRelation`, then call `.get()`.

use crate::change::Diff;
use crate::Tuple;

/// The core trait for relations.
/// A relation is a stream of changes - call foreach to iterate over pending changes.
pub trait Relation<T: Tuple>: 'static {
    /// Iterate over pending changes, calling f for each (tuple, count) pair.
    fn foreach(&mut self, f: &mut dyn FnMut(T, Diff));

    /// Box this relation to break the type chain.
    /// Use this when the compiler struggles with deeply nested types.
    fn boxed(self) -> Box<dyn Relation<T>>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

/// Implement Relation for Box<dyn Relation<T>> to allow type erasure.
impl<T: Tuple, R: Relation<T> + ?Sized> Relation<T> for Box<R> {
    fn foreach(&mut self, f: &mut dyn FnMut(T, Diff)) {
        (**self).foreach(f);
    }
}
