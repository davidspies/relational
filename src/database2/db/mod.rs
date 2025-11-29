//! The central Database2 type that coordinates commit, push/pop, and fixpoint.

mod wrappers;

use std::cell::RefCell;
use std::rc::Rc;

use crate::Tuple;

use super::feedback::Variable;
use super::relational::input::InputState;
use super::relational::{InputHandle, InputRelation, Relation};

use wrappers::{AnyFeedback, AnyInput, FeedbackWrapper, InputWrapper};

/// The main database type for coordinating differential dataflow.
pub struct Database2 {
    /// All registered inputs (type-erased).
    inputs: Vec<Box<dyn AnyInput>>,
    /// All registered feedbacks (type-erased).
    feedbacks: Vec<Box<dyn AnyFeedback>>,
    /// Current checkpoint stack depth.
    checkpoint_depth: usize,
    /// Maximum iterations for fixpoint.
    max_iterations: usize,
}

impl Database2 {
    /// Create a new empty database.
    pub fn new() -> Self {
        Database2 {
            inputs: Vec::new(),
            feedbacks: Vec::new(),
            checkpoint_depth: 0,
            max_iterations: 1000,
        }
    }

    /// Create an input and register it with the database.
    ///
    /// Returns a handle for inserting/deleting tuples and a relation for reading.
    /// Changes are staged until `db.commit()` is called.
    pub fn create_input<T: Tuple + 'static>(&mut self) -> (InputHandle<T>, InputRelation<T>) {
        let state = Rc::new(RefCell::new(InputState::new()));

        let handle = InputHandle {
            state: state.clone(),
        };
        let relation = InputRelation {
            state: state.clone(),
        };

        // Create wrapper for type-erased operations
        let wrapper = InputWrapper::new(state);
        wrapper.push_initial_checkpoints(self.checkpoint_depth);

        self.inputs.push(Box::new(wrapper));
        (handle, relation)
    }

    /// Commit staged changes and run stratified fixpoint.
    pub fn commit(&mut self) {
        // First, commit all inputs (staged -> pending)
        for input in &self.inputs {
            input.commit();
        }

        // Then run stratified fixpoint
        self.run_stratified_fixpoint();
    }

    /// Run stratified fixpoint computation for all feedbacks.
    fn run_stratified_fixpoint(&self) {
        let recording = self.checkpoint_depth > 0;
        let mut iterations = 0;

        'outer: loop {
            if iterations >= self.max_iterations {
                panic!(
                    "Stratified fixpoint exceeded max_iterations ({}) - possible infinite loop",
                    self.max_iterations
                );
            }

            for feedback in &self.feedbacks {
                if feedback.step(recording) {
                    iterations += 1;
                    continue 'outer;
                }
            }

            // No feedback produced new output, we're done
            break;
        }
    }

    /// Register a feedback: connect a variable to its input relation.
    ///
    /// The input relation computes new tuples to feed into the variable.
    /// During `commit()`, the database will run all feedbacks to fixpoint.
    pub fn feedback<T: Tuple + 'static, R: Relation<T> + 'static>(
        &mut self,
        variable: Rc<RefCell<Variable<T>>>,
        input: R,
    ) {
        let wrapper = FeedbackWrapper::new(variable, input);
        wrapper.push_initial_checkpoints(self.checkpoint_depth);
        self.feedbacks.push(Box::new(wrapper));
    }

    /// Push a new checkpoint level.
    pub fn push(&mut self) {
        self.checkpoint_depth += 1;
        for input in &self.inputs {
            input.push_checkpoint();
        }
        for feedback in &self.feedbacks {
            feedback.push_checkpoint();
        }
    }

    /// Pop a checkpoint level, undoing all changes since the matching push.
    pub fn pop(&mut self) -> bool {
        if self.checkpoint_depth == 0 {
            return false;
        }

        self.checkpoint_depth -= 1;

        // Step 1: Revert feedback outputs (emits -1 changes)
        for feedback in &self.feedbacks {
            feedback.pop_checkpoint();
        }

        // Step 2: Revert input changes (queues inverse diffs to pending)
        for input in &self.inputs {
            input.pop_checkpoint();
        }

        // Step 3: Commit to make inverse diffs available
        for input in &self.inputs {
            input.commit();
        }

        // Step 4: For each feedback in stratified order:
        // Pull input to update input_counts, readd, run fixpoint up to that point
        for i in 0..self.feedbacks.len() {
            self.feedbacks[i].pull_and_readd();
            self.run_stratified_fixpoint_up_to(i);
        }

        true
    }

    /// Run stratified fixpoint up to and including the given feedback index.
    fn run_stratified_fixpoint_up_to(&self, limit: usize) {
        let recording = self.checkpoint_depth > 0;
        let mut iterations = 0;

        'outer: loop {
            if iterations >= self.max_iterations {
                panic!(
                    "Stratified fixpoint exceeded max_iterations ({}) - possible infinite loop",
                    self.max_iterations
                );
            }

            for i in 0..=limit {
                if self.feedbacks[i].step(recording) {
                    iterations += 1;
                    continue 'outer;
                }
            }

            // No feedback produced new output, we're done
            break;
        }
    }

    /// Get the current checkpoint depth.
    pub fn depth(&self) -> usize {
        self.checkpoint_depth
    }

    /// Set maximum iterations for fixpoint.
    pub fn set_max_iterations(&mut self, max: usize) {
        self.max_iterations = max;
    }
}

impl Default for Database2 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
