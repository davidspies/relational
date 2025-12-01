//! Output - accumulates relation changes into a Sink.

use std::cell::RefCell;
use std::hash::Hash;
use std::marker::PhantomData;

use crate::collection::Multiset;
use crate::database::saved::SavedGetter;

use super::relation::{DynOp, Op, Relation};
use super::sink::Sink;

/// An output that accumulates changes from a relation into a Sink.
///
/// Changes accumulate - query methods automatically pull pending changes first.
/// Uses interior mutability so `get()` can take `&self`, allowing the caller
/// to hold references to multiple outputs simultaneously.
///
/// Default type parameters allow `Output<T>` as shorthand for boxed relations
/// with Multiset state.
pub struct Output<T, S = Multiset<T>, R = Box<dyn DynOp<T>>> {
    inner: RefCell<OutputInner<T, S, R>>,
}

pub type SavedOutput<T, S = Multiset<T>> = Output<T, S, SavedGetter<T, Box<dyn DynOp<T>>>>;

struct OutputInner<T, S, R> {
    relation: R,
    state: S,
    _phantom: PhantomData<T>,
}

impl<T, S: Sink<T>, R: Op<T>> Output<T, S, R> {
    /// Create a new output wrapping the given relation.
    pub(crate) fn new(relation: R) -> Self
    where
        S: Default,
    {
        Output {
            inner: RefCell::new(OutputInner {
                relation,
                state: S::default(),
                _phantom: PhantomData,
            }),
        }
    }

    /// Pull all pending changes from the relation into the accumulated state.
    fn update(inner: &mut OutputInner<T, S, R>) {
        inner.relation.foreach(|t, diff| {
            inner.state.apply(t, diff);
        });
    }

    /// Get a reference to the accumulated state after pulling pending changes.
    /// Uses interior mutability so this takes `&self` rather than `&mut self`.
    pub fn get(&self) -> std::cell::Ref<'_, S> {
        {
            let mut inner = self.inner.borrow_mut();
            Self::update(&mut inner);
        }
        std::cell::Ref::map(self.inner.borrow(), |inner| &inner.state)
    }
}

impl<T: Clone + Eq + Hash, R: Op<T>> Output<T, Multiset<T>, R> {
    /// Collect all tuples with positive multiplicity into a Vec.
    /// Automatically pulls pending changes first.
    pub fn collect(&self) -> Vec<T> {
        let mut inner = self.inner.borrow_mut();
        Output::<T, Multiset<T>, R>::update(&mut inner);
        inner.state.iter().cloned().collect()
    }
}

/// Create an output from a relation with Multiset state.
pub fn output<T: Eq + Hash, R: Op<T>>(relation: Relation<R>) -> Output<T, Multiset<T>, R> {
    Output::new(relation.inner)
}

/// Create an output from a relation with a custom sink type.
pub fn output_with_sink<T, S: Default + Sink<T>, R: Op<T>>(
    relation: Relation<R>,
) -> Output<T, S, R> {
    Output::new(relation.inner)
}
