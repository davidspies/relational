//! The central Database type that coordinates commit, push/pop, and fixpoint.

mod checkpoint;
mod fixpoint;
mod inputs;
mod wrappers;

use std::cell::Cell;
use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;

use super::commit_id::CommitId;
use super::relational::graph::{GraphBuilder, GraphHandle, new_graph_builder};
use super::relational::{Graph, Op, Relation, Variable, finalize_graph};

use wrappers::{
    AnyFeedback, AnyInput, AnyInterrupt, FeedbackWithIdWrapper, FeedbackWrapper, InterruptWrapper,
};

/// A step in the stratified fixpoint - either a feedback or an interrupt.
enum StratifiedStep {
    Feedback(Box<dyn AnyFeedback>),
    Interrupt(Box<dyn AnyInterrupt>),
}

/// The generic database type parameterized by graph storage.
///
/// `G` is either `GraphBuilder` (during construction) or `Arc<Graph>` (at runtime).
pub struct DatabaseG<G> {
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
    /// Graph storage - either mutable builder or immutable finalized graph.
    graph: G,
}

/// The builder for constructing a Database and its dataflow graph.
///
/// Use this to create inputs, variables, and set up feedback loops.
/// Call `build()` to finalize and get a `Database` for runtime operations.
pub type DatabaseBuilder = DatabaseG<GraphBuilder>;

/// The main database type for coordinating differential dataflow at runtime.
///
/// Created from a `DatabaseBuilder` via `build()`.
/// Use this for commit, push/pop, and accessing the dataflow graph.
pub type Database = DatabaseG<Arc<Graph>>;

impl DatabaseBuilder {
    /// Create a new empty database builder.
    pub fn new() -> Self {
        Self {
            inputs: Vec::new(),
            steps: Vec::new(),
            checkpoint_depth: 0,
            max_iterations: 1000,
            commit_id: Rc::new(Cell::new(CommitId::new(0))),
            graph: new_graph_builder(),
        }
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

    /// Finalize the builder and create a Database for runtime operations.
    ///
    /// After calling this, no more inputs, variables, or relations can be created.
    pub fn build(self) -> Database {
        let graph = finalize_graph(self.graph);
        Database {
            inputs: self.inputs,
            steps: self.steps,
            checkpoint_depth: self.checkpoint_depth,
            max_iterations: self.max_iterations,
            commit_id: self.commit_id,
            graph,
        }
    }
}

impl Default for DatabaseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Database {
    /// Get a handle to the shadow graph for visualization/debugging.
    pub fn graph(&self) -> GraphHandle {
        self.graph.clone()
    }

    /// Increment the commit ID counter.
    pub(super) fn increment_commit_id(&self) {
        let current_id = self.commit_id.get();
        self.commit_id.set(CommitId::new(current_id.raw() + 1));
    }
}

impl<G> DatabaseG<G> {
    /// Get the current commit ID.
    pub fn commit_id(&self) -> CommitId {
        self.commit_id.get()
    }
}

#[cfg(test)]
mod tests;
