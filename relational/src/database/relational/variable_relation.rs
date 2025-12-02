//! A Relation wrapper around a Variable for reading in feedback loops.

use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use crate::Diff;
use crate::database::feedback::Variable as InternalVariable;

use super::graph::NodeId;
use super::relation::Op;

/// A handle for a feedback variable.
///
/// Created by `Database::create_variable()` and passed to `Database::feedback()`
/// to wire up the input relation.
pub struct Variable<T> {
    pub(crate) inner: Rc<RefCell<InternalVariable<T>>>,
    /// The node ID of the VariableRelation in the dataflow graph.
    pub(crate) node_id: NodeId,
}

impl<T> Clone for Variable<T> {
    fn clone(&self) -> Self {
        Variable {
            inner: self.inner.clone(),
            node_id: self.node_id,
        }
    }
}

/// A relation that reads changes from a feedback variable.
///
/// Created by `Database::create_variable()`. Use this in your dataflow graph
/// to read the variable's output.
pub struct VariableRelation<T> {
    pub(crate) inner: Rc<RefCell<InternalVariable<T>>>,
}

impl<T: Clone + Eq + Hash> Op<T> for VariableRelation<T> {
    fn foreach(&mut self, mut consumer: impl FnMut(T, Diff)) {
        let mut var = self.inner.borrow_mut();
        for (tuple, diff) in var.drain_pending() {
            consumer(tuple, diff);
        }
    }
}
