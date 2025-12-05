//! Relation - a differential stream of changes.
//!
//! Relations are move-only (not Clone). To use a relation in multiple places,
//! you must first save it to get a `SavedRelation`, then call `.get()`.

use std::cell::Cell;
use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use contiguous_data::{Diff, Multiset};

use crate::database::commit_id::CommitId;

use super::graph::{GraphBuilder, NodeId};
use super::op::{DynOp, Op};

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

pub type BoxedRelation<T> = Relation<Box<dyn DynOp<T>>>;

impl<R> Relation<R> {
    /// Create a new relation with graph tracking.
    /// Panics if the graph has already been finalized.
    pub(crate) fn new(
        inner: R,
        commit_id: Rc<Cell<CommitId>>,
        graph: GraphBuilder,
        op_type: &'static str,
        parents: Vec<NodeId>,
    ) -> Self {
        let (node_id, counter) = graph
            .borrow_mut()
            .as_mut()
            .expect("cannot create relation after build()")
            .add_node(op_type, parents);
        Relation {
            inner,
            commit_id,
            graph,
            node_id,
            counter,
        }
    }

    pub(crate) fn modify_inner<RR>(self, f: impl FnOnce(R) -> RR) -> Relation<RR> {
        let Self {
            inner,
            commit_id,
            graph,
            node_id,
            counter,
        } = self;
        Relation {
            inner: f(inner),
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
    /// Panics if the graph has already been finalized.
    pub fn named(self, name: impl Into<String>) -> Self {
        self.graph
            .borrow_mut()
            .as_mut()
            .expect("cannot name relation after build()")
            .set_name(self.node_id, name.into());
        self
    }

    /// Override the op_type for this relation's graph node.
    /// Panics if the graph has already been finalized.
    pub(crate) fn with_op_type<T>(self, op_type: &'static str) -> Self
    where
        R: Op<T>,
    {
        if self.inner.passthrough_op_name(op_type) {
            return self;
        }
        self.set_op_type(op_type);
        self
    }

    pub(crate) fn set_op_type(&self, op_type: &'static str) {
        self.graph
            .borrow_mut()
            .as_mut()
            .expect("cannot set op_type after build()")
            .set_op_type(self.node_id, op_type);
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
