//! Relation - a differential stream of changes.
//!
//! Relations are move-only (not Clone). To use a relation in multiple places,
//! you must first save it to get a `SavedRelation`, then call `.get()`.

use std::cell::Cell;
use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::Multiset;
use crate::change::Diff;
use crate::database::commit_id::CommitId;

use super::graph::{GraphBuilder, NodeId};

/// The core trait for relational operators.
/// An operator is a stream of changes - call foreach to iterate over pending changes.
pub trait Op<T>: Sized {
    /// Iterate over pending changes, calling f for each (tuple, count) pair.
    fn foreach(&mut self, f: impl FnMut(T, Diff));

    fn dump_to_multiset(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash,
    {
        let mut counter = 0;
        self.foreach(|t, diff| {
            multiset.update(t, diff);
            counter += 1;
        });
        counter
    }

    /// Box this operator to allow type erasure.
    /// Use this when the compiler struggles with deeply nested types.
    fn boxed<'a>(self) -> Box<dyn DynOp<T> + 'a>
    where
        Self: 'a,
    {
        Box::new(self)
    }
}

pub trait DynOp<T> {
    fn foreach_dyn(&mut self, f: &mut dyn FnMut(T, Diff));

    fn dump_to_multiset_dyn(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash;
}

impl<T, R: Op<T>> DynOp<T> for R {
    fn foreach_dyn(&mut self, f: &mut dyn FnMut(T, Diff)) {
        self.foreach(f);
    }

    fn dump_to_multiset_dyn(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash,
    {
        self.dump_to_multiset(multiset)
    }
}

/// Implement Op for Box<dyn DynOp<T>> to allow type erasure.
impl<T> Op<T> for Box<dyn DynOp<T>> {
    fn foreach(&mut self, mut f: impl FnMut(T, Diff)) {
        (**self).foreach_dyn(&mut f);
    }

    fn dump_to_multiset(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash,
    {
        (**self).dump_to_multiset_dyn(multiset)
    }

    fn boxed<'a>(self) -> Box<dyn DynOp<T> + 'a> {
        self
    }
}

/// A relation wrapping a relational operator.
///
/// This wrapper allows attaching common associated data to all operators.
/// Use the `boxed()` method to break type chains when needed.
pub struct Relation<R> {
    pub(crate) inner: R,
    pub(crate) commit_id: Rc<Cell<CommitId>>,
    pub(crate) graph: GraphBuilder,
    pub(crate) node_id: NodeId,
    pub(crate) counter: Arc<AtomicUsize>,
}

impl<R> Relation<R> {
    /// Create a new relation with graph tracking.
    pub(crate) fn new(
        inner: R,
        commit_id: Rc<Cell<CommitId>>,
        graph: GraphBuilder,
        op_type: &'static str,
        parents: Vec<NodeId>,
    ) -> Self {
        let (node_id, counter) = graph.borrow_mut().add_node(op_type, parents);
        Relation {
            inner,
            commit_id,
            graph,
            node_id,
            counter,
        }
    }

    /// Get the node ID for this relation in the dataflow graph.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Give this relation a name for debugging/visualization.
    pub fn named(self, name: impl Into<String>) -> Self {
        self.graph.borrow_mut().set_name(self.node_id, name.into());
        self
    }

    /// Box this relation to break the type chain.
    /// Use this when the compiler struggles with deeply nested types.
    /// This doesn't create a new node in the graph - it reuses the parent's node.
    pub fn boxed<'a, T>(self) -> Relation<Box<dyn DynOp<T> + 'a>>
    where
        R: Op<T> + 'a,
    {
        Relation {
            inner: self.inner.boxed(),
            commit_id: self.commit_id,
            graph: self.graph,
            node_id: self.node_id,
            counter: self.counter,
        }
    }
}

/// Check that two commit IDs point to the same Rc.
/// Panics if they are different.
pub(crate) fn assert_same_commit_id(left: &Rc<Cell<CommitId>>, right: &Rc<Cell<CommitId>>) {
    assert!(
        Rc::ptr_eq(left, right),
        "Relations from different databases cannot be combined"
    );
}

impl<T, R: Op<T>> Op<T> for Relation<R> {
    fn foreach(&mut self, mut f: impl FnMut(T, Diff)) {
        let mut added = 0;
        self.inner.foreach(|t, diff| {
            f(t, diff);
            added += 1;
        });
        self.counter.fetch_add(added, Ordering::Relaxed);
    }

    fn dump_to_multiset(&mut self, multiset: &mut Multiset<T>) -> usize
    where
        T: Eq + Hash,
    {
        let added = self.inner.dump_to_multiset(multiset);
        self.counter.fetch_add(added, Ordering::Relaxed);
        added
    }
}
