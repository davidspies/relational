//! Relation - a differential stream of changes.
//!
//! Relations are move-only (not Clone). To use a relation in multiple places,
//! you must first save it to get a `SavedRelation`, then call `.get()`.

use crate::change::Diff;

/// The core trait for relational operators.
/// An operator is a stream of changes - call foreach to iterate over pending changes.
pub trait Op<T> {
    /// Iterate over pending changes, calling f for each (tuple, count) pair.
    fn foreach(&mut self, f: &mut dyn FnMut(T, Diff));
}

/// Implement Op for Box<dyn Op<T>> to allow type erasure.
impl<T, R: Op<T> + ?Sized> Op<T> for Box<R> {
    fn foreach(&mut self, f: &mut dyn FnMut(T, Diff)) {
        (**self).foreach(f);
    }
}

/// A relation wrapping a relational operator.
///
/// This wrapper allows attaching common associated data to all operators.
/// Use the `boxed()` method to break type chains when needed.
pub struct Relation<R> {
    pub(crate) inner: R,
}

impl<R> Relation<R> {
    /// Create a new relation from an operator.
    pub fn new(inner: R) -> Self {
        Relation { inner }
    }

    /// Box this relation to break the type chain.
    /// Use this when the compiler struggles with deeply nested types.
    pub fn boxed<'a, T>(self) -> Relation<Box<dyn Op<T> + 'a>>
    where
        R: Op<T> + 'a,
    {
        Relation::new(Box::new(self.inner))
    }
}

impl<T, R: Op<T>> Op<T> for Relation<R> {
    fn foreach(&mut self, f: &mut dyn FnMut(T, Diff)) {
        self.inner.foreach(f);
    }
}
