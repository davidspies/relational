//! The central Database2 type that coordinates commit, push/pop, and fixpoint.

mod wrappers;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::Tuple;

use super::commit_id::CommitId;
use super::feedback::Variable as InternalVariable;
use super::relational::input::InputState;
use super::relational::{
    InputHandle, PersistentInputHandle, Relation, Variable, VariableRelation,
    input::InputRelation,
};

use wrappers::{
    AnyFeedback, AnyInput, AnyInterrupt, FeedbackWithIdWrapper, FeedbackWrapper, InputWrapper,
    InterruptWrapper,
};

/// The main database type for coordinating differential dataflow.
pub struct Database2 {
    /// All registered inputs (type-erased).
    inputs: Vec<Box<dyn AnyInput>>,
    /// All registered feedbacks (type-erased).
    feedbacks: Vec<Box<dyn AnyFeedback>>,
    /// All registered interrupts (type-erased).
    interrupts: Vec<Box<dyn AnyInterrupt>>,
    /// Current checkpoint stack depth.
    checkpoint_depth: usize,
    /// Maximum iterations for fixpoint.
    max_iterations: usize,
    /// Shared commit ID counter for feedback_with_id.
    commit_id: Rc<Cell<CommitId>>,
    /// Whether the last fixpoint was interrupted.
    was_interrupted: bool,
}

impl Database2 {
    /// Create a new empty database.
    pub fn new() -> Self {
        Database2 {
            inputs: Vec::new(),
            feedbacks: Vec::new(),
            interrupts: Vec::new(),
            checkpoint_depth: 0,
            max_iterations: 1000,
            commit_id: Rc::new(Cell::new(CommitId::new(0))),
            was_interrupted: false,
        }
    }

    /// Get the current commit ID.
    pub(crate) fn commit_id(&self) -> CommitId {
        self.commit_id.get()
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

    /// Create a persistent input that survives pop().
    ///
    /// Like `create_input`, but changes are not undone when `pop()` is called.
    /// Use this for learned clauses, facts that should persist through backtracking, etc.
    ///
    /// Returns a `PersistentInputHandle` which supports both insert and delete.
    pub fn create_persistent_input<T: Tuple + 'static>(
        &mut self,
    ) -> (PersistentInputHandle<T>, InputRelation<T>) {
        let state = Rc::new(RefCell::new(InputState::new()));

        let handle = PersistentInputHandle {
            state: state.clone(),
        };
        let relation = InputRelation {
            state: state.clone(),
        };

        (handle, relation)
    }

    /// Create a feedback variable.
    ///
    /// Returns a `Variable` handle (to pass to `feedback()`) and a `VariableRelation`
    /// (to use in your dataflow graph).
    ///
    /// # Example
    /// ```ignore
    /// let (var, var_rel) = db.create_variable::<i32>();
    /// let derived = map(var_rel, |x| x * 2);
    /// db.feedback(var, some_input_relation);
    /// ```
    pub fn create_variable<T: Tuple + 'static>(&self) -> (Variable<T>, VariableRelation<T>) {
        let inner = Rc::new(RefCell::new(InternalVariable::new()));
        let var = Variable {
            inner: inner.clone(),
        };
        let rel = VariableRelation { inner };
        (var, rel)
    }

    /// Commit staged changes and run stratified fixpoint.
    pub fn commit(&mut self) {
        // Reset interrupt flag
        self.was_interrupted = false;

        // Record any pending inserts to the current checkpoint level
        for input in &self.inputs {
            input.record_pending_inserts();
        }

        // Then run stratified fixpoint (feedbacks commit in step())
        self.run_stratified_fixpoint();
    }

    /// Run stratified fixpoint computation for all feedbacks.
    fn run_stratified_fixpoint(&mut self) {
        let recording = self.checkpoint_depth > 0;
        let mut iterations = 0;

        'outer: loop {
            if iterations >= self.max_iterations {
                panic!(
                    "Stratified fixpoint exceeded max_iterations ({}) - possible infinite loop",
                    self.max_iterations
                );
            }

            // Check interrupts first
            for interrupt in &mut self.interrupts {
                if interrupt.check() {
                    self.was_interrupted = true;
                    return;
                }
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
    /// This immediately runs stratified fixpoint to compute initial values.
    pub fn feedback<T: Tuple + 'static, R: Relation<T> + 'static>(
        &mut self,
        variable: Variable<T>,
        input: R,
    ) {
        let wrapper = FeedbackWrapper::new(variable.inner, input);
        wrapper.push_initial_checkpoints(self.checkpoint_depth);
        self.feedbacks.push(Box::new(wrapper));

        // Run fixpoint immediately (like old Database did)
        self.run_stratified_fixpoint();
    }

    /// Register a feedback that tracks discovery time.
    ///
    /// Like `feedback`, but the variable holds `(T, CommitId)` where the CommitId
    /// records when each tuple was first discovered. This allows deriving relations
    /// that depend on discovery order (e.g., taking the tuple with minimum CommitId).
    ///
    /// The `input` is `Relation<T>`, but the variable holds `(T, CommitId)`.
    /// When a tuple T is first seen, it's added to the variable with the current commit ID.
    pub fn feedback_with_id<T: Tuple + 'static, R: Relation<T> + 'static>(
        &mut self,
        variable: Variable<(T, CommitId)>,
        input: R,
    ) {
        let wrapper = FeedbackWithIdWrapper::new(variable.inner, input, self.commit_id.clone());
        wrapper.push_initial_checkpoints(self.checkpoint_depth);
        self.feedbacks.push(Box::new(wrapper));
    }

    /// Register an interrupt that stops fixpoint when the relation becomes non-empty.
    ///
    /// If the relation produces any positive tuples during fixpoint propagation,
    /// the fixpoint stops immediately. Check `was_interrupted()` after `commit()`
    /// to see if an interrupt fired.
    pub fn interrupt<T: Tuple + 'static, R: Relation<T> + 'static>(&mut self, input: R) {
        self.interrupts.push(Box::new(InterruptWrapper::new(input)));
    }

    /// Check if the last fixpoint was interrupted.
    pub fn was_interrupted(&self) -> bool {
        self.was_interrupted
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
    ///
    /// Algorithm:
    /// 1. Unapply non-persistent input AND feedback changes by sending -1's
    ///    - For non-persistent inputs, also pop the checkpoint from the stack
    ///    - Don't pop for feedbacks yet
    /// 2. Commit changes
    /// 3. In stratified order of feedbacks, for each feedback:
    ///    a) Pull all changes and update tracked inputs; forward along anything
    ///       which is NOT in the last checkpoint with a +1
    ///    b) Pop the last checkpoint; forward along a +1 for anything in that
    ///       checkpoint whose tracked input value is still non-zero
    ///    c) Propagate normally all feedbacks up to and including this one
    ///       (using nested loop approach where you restart from beginning if changes)
    pub fn pop(&mut self) -> bool {
        if self.checkpoint_depth == 0 {
            return false;
        }

        self.checkpoint_depth -= 1;

        // Step 1: Unapply non-persistent input AND feedback changes by sending -1's
        // For non-persistent inputs, also pop the checkpoint from the stack
        for input in &self.inputs {
            input.send_inverse_and_pop();
        }
        // For feedbacks, send -1's but don't pop yet
        for feedback in &self.feedbacks {
            feedback.send_inverse();
        }

        // Step 2: Commit feedback changes so they can be pulled
        for feedback in &self.feedbacks {
            feedback.commit();
        }

        // Step 3: In stratified order of feedbacks, for each feedback:
        for i in 0..self.feedbacks.len() {
            // 3a) Pull all changes and update tracked inputs; forward along anything
            //     which is NOT in the last checkpoint with a +1
            self.feedbacks[i].pull_and_forward_non_checkpoint();

            // 3b) Pop the last checkpoint; forward along a +1 for anything in that
            //     checkpoint whose tracked input value is still non-zero
            self.feedbacks[i].pop_and_forward_reachable();

            // 3c) Propagate normally all feedbacks up to and including this one
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
    pub(crate) fn set_max_iterations(&mut self, max: usize) {
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
