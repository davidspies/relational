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
    relation: Relation<R>,
    scratch: Multiset<T>,
    state: S,
    _phantom: PhantomData<T>,
}

impl<T: Eq + Hash, S: Sink<T>, R: Op<T>> OutputInner<T, S, R> {
    /// Pull all pending changes from the relation into the accumulated state.
    fn update(&mut self) {
        self.relation.dump_to_multiset(&mut self.scratch);
        self.state.dump_all(&mut self.scratch);
    }
}

impl<T: Eq + Hash, S: Sink<T>, R: Op<T>> Output<T, S, R> {
    /// Create a new output wrapping the given relation.
    pub(crate) fn new(relation: Relation<R>) -> Self
    where
        S: Default,
    {
        // Add an output node to the graph (no counter for terminal nodes)
        let parent_id = relation.node_id;
        if let Some(graph) = relation.graph.borrow_mut().as_mut() {
            graph.add_terminal_node("output", vec![parent_id]);
        }

        Output {
            inner: RefCell::new(OutputInner {
                relation,
                scratch: Multiset::new(),
                state: S::default(),
                _phantom: PhantomData,
            }),
        }
    }

    /// Get a reference to the accumulated state after pulling pending changes.
    /// Uses interior mutability so this takes `&self` rather than `&mut self`.
    pub fn get(&self) -> std::cell::Ref<'_, S> {
        self.inner.borrow_mut().update();
        std::cell::Ref::map(self.inner.borrow(), |inner| &inner.state)
    }
}

impl<T: Clone + Eq + Hash, R: Op<T>> Output<T, Multiset<T>, R> {
    /// Collect all tuples with positive multiplicity into a Vec.
    /// Automatically pulls pending changes first.
    pub fn collect(&self) -> Vec<T> {
        let mut inner = self.inner.borrow_mut();
        inner.update();
        inner.state.iter().cloned().collect()
    }
}

impl<R> Relation<R> {
    /// Create an output from a relation with Multiset state.
    pub fn output<T: Eq + Hash>(self) -> Output<T, Multiset<T>, R>
    where
        R: Op<T>,
    {
        Output::new(self)
    }

    /// Create an output from a relation with a custom sink type.
    pub fn output_with_sink<T: Eq + Hash, S: Default + Sink<T>>(self) -> Output<T, S, R>
    where
        R: Op<T>,
    {
        Output::new(self)
    }
}
