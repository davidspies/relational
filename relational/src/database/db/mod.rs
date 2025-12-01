//! The central Database type that coordinates commit, push/pop, and fixpoint.

mod checkpoint;
mod fixpoint;
mod inputs;
mod wrappers;

use std::cell::Cell;
use std::hash::Hash;
use std::rc::Rc;

use super::commit_id::CommitId;
use super::relational::graph::{GraphBuilder, GraphHandle, new_graph_builder};
use super::relational::{Op, Relation, Variable};

use wrappers::{
    AnyFeedback, AnyInput, AnyInterrupt, FeedbackWithIdWrapper, FeedbackWrapper, InterruptWrapper,
};

/// A step in the stratified fixpoint - either a feedback or an interrupt.
enum StratifiedStep {
    Feedback(Box<dyn AnyFeedback>),
    Interrupt(Box<dyn AnyInterrupt>),
}

/// The main database type for coordinating differential dataflow.
pub struct Database {
    /// All registered inputs (type-erased).
    inputs: Vec<Box<dyn AnyInput>>,
    /// Stratified steps (feedbacks and interrupts) in registration order.
    steps: Vec<StratifiedStep>,
    /// Current checkpoint stack depth.
    checkpoint_depth: usize,
    /// Maximum iterations for fixpoint.
    max_iterations: usize,
    /// Shared commit ID counter for feedback_with_id.
    commit_id: Rc<Cell<CommitId>>,
    /// Shadow graph for tracking dataflow structure and element counts.
    graph: GraphBuilder,
}

impl Database {
    /// Create a new empty database.
    pub fn new() -> Self {
        Database {
            inputs: Vec::new(),
            steps: Vec::new(),
            checkpoint_depth: 0,
            max_iterations: 1000,
            commit_id: Rc::new(Cell::new(CommitId::new(0))),
            graph: new_graph_builder(),
        }
    }

    /// Get a handle to the shadow graph for visualization/debugging.
    pub fn graph(&self) -> GraphHandle {
        std::sync::Arc::new(self.graph.borrow().clone())
    }

    /// Increment the commit ID counter.
    pub(super) fn increment_commit_id(&self) {
        let current_id = self.commit_id.get();
        self.commit_id.set(CommitId::new(current_id.raw() + 1));
    }

    /// Register a feedback: connect a variable to its input relation.
    ///
    /// The input relation computes new tuples to feed into the variable.
    /// This immediately runs stratified fixpoint to compute initial values.
    pub fn feedback<T: Clone + Eq + Hash + 'static, R: Op<T> + 'static>(
        &mut self,
        variable: Variable<T>,
        input: Relation<R>,
    ) {
        let mut wrapper = FeedbackWrapper::new(variable.inner, input);
        wrapper.push_initial_checkpoints(self.checkpoint_depth);
        self.steps.push(StratifiedStep::Feedback(Box::new(wrapper)));
        self.run_stratified_fixpoint();
    }

    /// Register a feedback that tracks discovery time.
    ///
    /// Like `feedback`, but the variable holds `(T, CommitId)` where the CommitId
    /// records when each tuple was first discovered.
    pub fn feedback_with_id<T: Clone + Eq + Hash + 'static, R: Op<T> + 'static>(
        &mut self,
        variable: Variable<(T, CommitId)>,
        input: Relation<R>,
    ) {
        let mut wrapper = FeedbackWithIdWrapper::new(variable.inner, input, self.commit_id.clone());
        wrapper.push_initial_checkpoints(self.checkpoint_depth);
        self.steps.push(StratifiedStep::Feedback(Box::new(wrapper)));
    }

    /// Register an interrupt that stops fixpoint when the relation becomes non-empty.
    pub fn interrupt<T: 'static, R: Op<T> + 'static>(&mut self, input: Relation<R>) {
        self.steps
            .push(StratifiedStep::Interrupt(Box::new(InterruptWrapper::new(
                input,
            ))));
    }

    /// Get the current commit ID.
    pub fn commit_id(&self) -> CommitId {
        self.commit_id.get()
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
