//! The central Database type that coordinates commit, push/pop, and fixpoint.

mod wrappers;

use std::cell::{Cell, RefCell};
use std::hash::Hash;
use std::rc::Rc;

use super::commit_id::CommitId;
use super::feedback::Variable as InternalVariable;
use super::relational::input::InputState;
use super::relational::saved::SavedRelation;
use super::relational::{
    InputHandle, Op, PersistentInputHandle, Relation, Variable, VariableRelation,
    input::InputRelation,
};

use wrappers::{
    AnyFeedback, AnyInput, AnyInterrupt, FeedbackWithIdWrapper, FeedbackWrapper, InputWrapper,
    InterruptWrapper,
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
        }
    }

    /// Create an input and register it with the database.
    ///
    /// Returns a handle for inserting/deleting tuples and a relation for reading.
    /// Changes are staged until `db.commit()` is called.
    pub fn create_input<T: Clone + Eq + Hash + 'static>(
        &mut self,
    ) -> (InputHandle<T>, Relation<InputRelation<T>>) {
        let state = Rc::new(RefCell::new(InputState::new()));

        let handle = InputHandle {
            state: state.clone(),
        };
        let relation = Relation::new(InputRelation {
            state: state.clone(),
        });

        // Create wrapper for type-erased operations
        let mut wrapper = InputWrapper::new(state);
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
    pub fn create_persistent_input<T: Clone + Eq + Hash>(
        &mut self,
    ) -> (PersistentInputHandle<T>, Relation<InputRelation<T>>) {
        let state = Rc::new(RefCell::new(InputState::new()));

        let handle = PersistentInputHandle {
            state: state.clone(),
        };
        let relation = Relation::new(InputRelation {
            state: state.clone(),
        });

        (handle, relation)
    }

    /// Create a feedback variable.
    ///
    /// Returns a `Variable` handle (to pass to `feedback()`) and a `Relation<VariableRelation>`
    /// (to use in your dataflow graph).
    ///
    /// # Example
    /// ```ignore
    /// let (var, var_rel) = db.create_variable::<i32>();
    /// let derived = map(var_rel, |x| x * 2);
    /// db.feedback(var, some_input_relation);
    /// ```
    pub fn create_variable<T: Clone + Eq + Hash>(
        &self,
    ) -> (Variable<T>, Relation<VariableRelation<T>>) {
        let inner = Rc::new(RefCell::new(InternalVariable::new()));
        let var = Variable {
            inner: inner.clone(),
        };
        let rel = Relation::new(VariableRelation { inner });
        (var, rel)
    }

    /// Commit staged changes and run stratified fixpoint.
    pub fn commit(&mut self) {
        // Increment commit ID for this commit
        let current_id = self.commit_id.get();
        let new_id = CommitId::new(current_id.raw() + 1);
        self.commit_id.set(new_id);

        // Record any pending inserts to the current checkpoint level
        for input in &mut self.inputs {
            input.record_pending_inserts();
        }

        // Then run stratified fixpoint (feedbacks commit in step())
        self.run_stratified_fixpoint();
    }

    /// Run stratified fixpoint computation for all steps (feedbacks and interrupts).
    fn run_stratified_fixpoint(&mut self) {
        if !self.steps.is_empty() {
            self.run_stratified_fixpoint_up_to(self.steps.len() - 1);
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
        let mut wrapper = FeedbackWrapper::new(variable.inner, input.inner, self.commit_id.clone());
        wrapper.push_initial_checkpoints(self.checkpoint_depth);
        self.steps.push(StratifiedStep::Feedback(Box::new(wrapper)));

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
    pub fn feedback_with_id<T: Clone + Eq + Hash + 'static, R: Op<T> + 'static>(
        &mut self,
        variable: Variable<(T, CommitId)>,
        input: Relation<R>,
    ) {
        let mut wrapper =
            FeedbackWithIdWrapper::new(variable.inner, input.inner, self.commit_id.clone());
        wrapper.push_initial_checkpoints(self.checkpoint_depth);
        self.steps.push(StratifiedStep::Feedback(Box::new(wrapper)));
    }

    /// Register an interrupt that stops fixpoint when the relation becomes non-empty.
    ///
    /// If the relation produces any positive tuples during fixpoint propagation,
    /// the fixpoint stops immediately. Check `was_interrupted()` after `commit()`
    /// to see if an interrupt fired.
    pub fn interrupt<T: 'static, R: Op<T> + 'static>(&mut self, input: Relation<R>) {
        self.steps
            .push(StratifiedStep::Interrupt(Box::new(InterruptWrapper::new(
                input.inner,
            ))));
    }

    /// Create a saved relation that can be used in multiple places.
    ///
    /// This is an optimized version of the standalone `save()` function that
    /// tracks the database's commit ID to avoid redundant upstream pulls.
    pub fn save<T: Eq + Hash, R: Op<T>>(&self, upstream: Relation<R>) -> SavedRelation<T, R> {
        SavedRelation::with_commit_id(upstream.inner, self.commit_id.clone())
    }

    /// Push a new checkpoint level.
    pub fn push(&mut self) {
        self.checkpoint_depth += 1;
        for input in &mut self.inputs {
            input.push_checkpoint();
        }
        for step in &mut self.steps {
            if let StratifiedStep::Feedback(feedback) = step {
                feedback.push_checkpoint();
            }
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
    ///    which is NOT in the last checkpoint with a +1
    ///    b) Pop the last checkpoint; forward along a +1 for anything in that
    ///    checkpoint whose tracked input value is still non-zero
    ///    c) Propagate normally all feedbacks up to and including this one
    ///    (using nested loop approach where you restart from beginning if changes)
    #[must_use]
    pub fn pop(&mut self) -> bool {
        if self.checkpoint_depth == 0 {
            return false;
        }

        self.checkpoint_depth -= 1;

        // Step 1: Unapply non-persistent input AND feedback changes by sending -1's
        // For non-persistent inputs, also pop the checkpoint from the stack
        for input in &mut self.inputs {
            input.send_inverse_and_pop();
        }
        // For feedbacks, send -1's but don't pop yet
        for step in &mut self.steps {
            if let StratifiedStep::Feedback(feedback) = step {
                feedback.send_inverse();
            }
        }

        // Step 2: Commit feedback changes so they can be pulled
        for step in &mut self.steps {
            if let StratifiedStep::Feedback(feedback) = step {
                feedback.commit();
            }
        }

        // Step 3: In stratified order, for each feedback:
        //   - pull_and_forward_non_checkpoint, pop_and_forward_reachable, commit
        //   - Then run fixpoint up to this step
        // If an interrupt fires, continue processing feedbacks

        for i in 0..self.steps.len() {
            if let StratifiedStep::Feedback(feedback) = &mut self.steps[i] {
                // 3a) Pull all changes and update tracked inputs; forward along anything
                //     which is NOT in the last checkpoint with a +1
                feedback.pull_and_forward_non_checkpoint();

                // 3b) Pop the last checkpoint; forward along a +1 for anything in that
                //     checkpoint whose tracked input value is still non-zero
                feedback.pop_and_forward_reachable();

                // Commit the forwarded changes so they can be pulled
                feedback.commit();

                // 3c) Propagate normally all steps up to and including this one
                self.run_stratified_fixpoint_up_to(i);
            }
        }

        // Reset all interrupts so they don't keep firing on subsequent commits
        for step in &mut self.steps {
            if let StratifiedStep::Interrupt(interrupt) = step {
                interrupt.reset();
            }
        }

        true
    }

    /// Run stratified fixpoint up to and including the given step index.
    /// Returns Some(step_index) if an interrupt fired, None otherwise.
    fn run_stratified_fixpoint_up_to(&mut self, limit: usize) {
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
                match &mut self.steps[i] {
                    StratifiedStep::Interrupt(interrupt) => {
                        if interrupt.check() {
                            return;
                        }
                    }
                    StratifiedStep::Feedback(feedback) => {
                        if feedback.step(recording) {
                            iterations += 1;
                            continue 'outer;
                        }
                    }
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

    /// Get the current commit ID.
    ///
    /// This is a monotonically increasing counter that advances with each
    /// feedback iteration. It never decreases, even during backtracking.
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
