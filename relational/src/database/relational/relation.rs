//! Relation - a differential stream of changes.
//!
//! Relations are move-only (not Clone). To use a relation in multiple places,
//! you must first save it to get a `SavedRelation`, then call `.get()`.

use crate::change::Diff;

/// The core trait for relations.
/// A relation is a stream of changes - call foreach to iterate over pending changes.
pub trait Op<T> {
    /// Iterate over pending changes, calling f for each (tuple, count) pair.
    fn foreach(&mut self, f: &mut dyn FnMut(T, Diff));

    /// Box this relation to break the type chain.
    /// Use this when the compiler struggles with deeply nested types.
    fn boxed<'a>(self) -> Box<dyn Op<T> + 'a>
    where
        Self: Sized + 'a,
    {
        Box::new(self)
    }
}

/// Implement Relation for Box<dyn Relation<T>> to allow type erasure.
impl<T, R: Op<T> + ?Sized> Op<T> for Box<R> {
    fn foreach(&mut self, f: &mut dyn FnMut(T, Diff)) {
        (**self).foreach(f);
    }
}
