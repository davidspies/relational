//! Relation - a differential stream of changes.
//!
//! Relations are move-only (not Clone). To use a relation in multiple places,
//! you must first save it to get a `SavedRelation`, then call `.get()`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::change::Diff;
use crate::database::commit_id::CommitId;

use super::graph::{GraphBuilder, NodeId};

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
    pub fn boxed<'a, T>(self) -> Relation<Box<dyn Op<T> + 'a>>
    where
        R: Op<T> + 'a,
    {
        let (node_id, counter) = self
            .graph
            .borrow_mut()
            .add_node("boxed", vec![self.node_id]);
        Relation {
            inner: Box::new(self.inner),
            commit_id: self.commit_id,
            graph: self.graph,
            node_id,
            counter,
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
    fn foreach(&mut self, f: &mut dyn FnMut(T, Diff)) {
        let counter = self.counter.clone();
        self.inner.foreach(&mut |t, diff| {
            counter.fetch_add(1, Ordering::Relaxed);
            f(t, diff);
        });
    }
}
