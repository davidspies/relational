//! Type-erased wrappers for database-managed components.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::Tuple;
use crate::change::Diff;
use crate::database2::commit_id::CommitId;
use crate::database2::feedback::Variable;
use crate::database2::relational::Relation;
use crate::database2::relational::input::InputState;

/// Type-erased input handle operations.
pub(super) trait AnyInput {
    /// Push a checkpoint - start recording inserts.
    fn push_checkpoint(&mut self);
    /// Record any new inserts (from pending) to the current checkpoint.
    /// Called during db.commit() to capture what was inserted.
    fn record_pending_inserts(&mut self);
    /// Remove all tuples inserted in the current checkpoint and pop.
    /// Used as step 1 of the database pop() algorithm.
    fn send_inverse_and_pop(&mut self);
}

/// Wrapper to make InputHandle type-erased (seen-set semantics).
pub(super) struct InputWrapper<T: Tuple> {
    /// Shared state with InputHandle and InputRelation.
    state: Rc<RefCell<InputState<T>>>,
    /// Stack of tuples inserted at each checkpoint level.
    checkpoint_stack: Vec<HashSet<T>>,
}

impl<T: Tuple> InputWrapper<T> {
    pub(super) fn new(state: Rc<RefCell<InputState<T>>>) -> Self {
        InputWrapper {
            state,
            checkpoint_stack: Vec::new(),
        }
    }

    pub(super) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.checkpoint_stack.push(HashSet::new());
        }
    }
}

impl<T: Tuple + 'static> AnyInput for InputWrapper<T> {
    fn push_checkpoint(&mut self) {
        self.checkpoint_stack.push(HashSet::new());
    }

    fn record_pending_inserts(&mut self) {
        if let Some(level) = self.checkpoint_stack.last_mut() {
            let new_inserts = self.state.borrow_mut().take_new_inserts();
            level.extend(new_inserts);
        } else {
            self.state.borrow_mut().take_new_inserts();
        }
    }

    fn send_inverse_and_pop(&mut self) {
        if let Some(tuples) = self.checkpoint_stack.pop() {
            let mut state = self.state.borrow_mut();
            for tuple in tuples {
                state.remove(&tuple);
            }
        }
    }
}

/// Type-erased feedback operations.
pub(super) trait AnyFeedback {
    /// Push a checkpoint.
    fn push_checkpoint(&mut self);
    /// Send -1 for all outputs in the current checkpoint (don't pop yet).
    fn send_inverse(&mut self);
    /// Commit the variable's current changes so they can be pulled by downstream.
    fn commit(&mut self);
    /// Pull changes, update tracked inputs, forward +1 for items not in last checkpoint.
    fn pull_and_forward_non_checkpoint(&mut self);
    /// Pop checkpoint, forward +1 for items whose tracked input is still positive.
    fn pop_and_forward_reachable(&mut self);
    /// Run one step: pull from input relation, add to variable. Returns true if new output.
    fn step(&mut self, recording: bool) -> bool;
}

/// Wrapper to make feedback type-erased.
pub(super) struct FeedbackWrapper<T: Tuple, R: Relation<T>> {
    /// Shared variable state (also accessed by VariableRelation).
    variable: Rc<RefCell<Variable<T>>>,
    /// The input relation that feeds into this variable.
    input: R,
}

impl<T: Tuple, R: Relation<T>> FeedbackWrapper<T, R> {
    pub(super) fn new(variable: Rc<RefCell<Variable<T>>>, input: R) -> Self {
        FeedbackWrapper { variable, input }
    }

    pub(super) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.variable.borrow_mut().push_checkpoint();
        }
    }
}

impl<T: Tuple + 'static, R: Relation<T> + 'static> AnyFeedback for FeedbackWrapper<T, R> {
    fn push_checkpoint(&mut self) {
        self.variable.borrow_mut().push_checkpoint();
    }

    fn send_inverse(&mut self) {
        self.variable.borrow_mut().send_inverse();
    }

    fn commit(&mut self) {
        self.variable.borrow_mut().commit();
    }

    fn pull_and_forward_non_checkpoint(&mut self) {
        let mut changes = Vec::new();
        self.input.foreach(&mut |tuple: T, diff: Diff| {
            changes.push((tuple, diff));
        });

        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in changes {
            var.update_input_total(tuple.clone(), diff);
            var.forward_if_not_in_checkpoint(&tuple);
        }
    }

    fn pop_and_forward_reachable(&mut self) {
        self.variable.borrow_mut().pop_and_forward_reachable();
    }

    fn step(&mut self, _recording: bool) -> bool {
        let mut changes = Vec::new();
        self.input.foreach(&mut |tuple: T, diff: Diff| {
            changes.push((tuple, diff));
        });

        if changes.is_empty() {
            return false;
        }

        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in changes {
            var.add_change(tuple, diff);
        }
        var.commit();

        true
    }
}

/// Type-erased interrupt operations.
pub(super) trait AnyInterrupt {
    /// Check if the interrupt condition is met (relation has positive entries).
    fn check(&mut self) -> bool;
    /// Reset the interrupt state (clear has_positive flag).
    fn reset(&mut self);
}

/// Wrapper for interrupt - checks if a relation has any positive entries.
pub(super) struct InterruptWrapper<T: Tuple, R: Relation<T>> {
    input: R,
    has_positive: bool,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Tuple, R: Relation<T>> InterruptWrapper<T, R> {
    pub(super) fn new(input: R) -> Self {
        InterruptWrapper {
            input,
            has_positive: false,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T: Tuple + 'static, R: Relation<T> + 'static> AnyInterrupt for InterruptWrapper<T, R> {
    fn check(&mut self) -> bool {
        self.input.foreach(&mut |_, diff| {
            if diff.0 > 0 {
                self.has_positive = true;
            }
        });
        self.has_positive
    }

    fn reset(&mut self) {
        self.has_positive = false;
    }
}

/// Wrapper for feedback_with_id - stamps tuples with CommitId when first seen.
/// Input relation produces T, variable stores (T, CommitId).
pub(super) struct FeedbackWithIdWrapper<T: Tuple, R: Relation<T>> {
    /// Shared variable state (also accessed by VariableRelation).
    variable: Rc<RefCell<Variable<(T, CommitId)>>>,
    /// Shared commit ID counter.
    commit_id: Rc<Cell<CommitId>>,
    /// The input relation produces T.
    input: R,
    /// Track input totals by T alone (not (T, CommitId)) for pop() handling.
    input_totals_by_t: HashMap<T, i64>,
    /// Maps T -> CommitId for tuples currently in output.
    t_to_commit_id: HashMap<T, CommitId>,
}

impl<T: Tuple, R: Relation<T>> FeedbackWithIdWrapper<T, R> {
    pub(super) fn new(
        variable: Rc<RefCell<Variable<(T, CommitId)>>>,
        input: R,
        commit_id: Rc<Cell<CommitId>>,
    ) -> Self {
        FeedbackWithIdWrapper {
            variable,
            commit_id,
            input,
            input_totals_by_t: HashMap::new(),
            t_to_commit_id: HashMap::new(),
        }
    }

    pub(super) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.variable.borrow_mut().push_checkpoint();
        }
    }
}

impl<T: Tuple + 'static, R: Relation<T> + 'static> AnyFeedback for FeedbackWithIdWrapper<T, R> {
    fn push_checkpoint(&mut self) {
        self.variable.borrow_mut().push_checkpoint();
    }

    fn send_inverse(&mut self) {
        self.variable.borrow_mut().send_inverse();
    }

    fn commit(&mut self) {
        self.variable.borrow_mut().commit();
    }

    fn pull_and_forward_non_checkpoint(&mut self) {
        let mut changes = Vec::new();
        self.input.foreach(&mut |tuple: T, diff: Diff| {
            changes.push((tuple, diff));
        });

        // Update our T-keyed input_totals
        for (tuple, diff) in &changes {
            *self.input_totals_by_t.entry(tuple.clone()).or_insert(0) += diff.0;
        }

        // For each T that changed, look up the actual (T, CommitId) in our mapping
        // and forward if not in checkpoint
        let mut var = self.variable.borrow_mut();
        for (tuple, _) in changes {
            if let Some(&commit_id) = self.t_to_commit_id.get(&tuple) {
                let full_tuple = (tuple, commit_id);
                // Update the variable's input_total for the full tuple
                let input_total = self.input_totals_by_t.get(&full_tuple.0).copied().unwrap_or(0);
                // We need to sync the variable's view - set it to match our tracking
                var.set_input_total(full_tuple.clone(), input_total);
                var.forward_if_not_in_checkpoint(&full_tuple);
            }
        }
    }

    fn pop_and_forward_reachable(&mut self) {
        // Get the checkpoint contents before popping
        let checkpoint_tuples: Vec<(T, CommitId)> = {
            let var = self.variable.borrow();
            var.get_last_checkpoint().to_vec()
        };

        // Pop the checkpoint
        self.variable.borrow_mut().pop_checkpoint();

        // For each tuple in the checkpoint, check if it's still reachable
        let mut var = self.variable.borrow_mut();
        for (tuple, commit_id) in checkpoint_tuples {
            let input_total = self.input_totals_by_t.get(&tuple).copied().unwrap_or(0);
            if input_total > 0 {
                // Still reachable - re-add to output
                var.forward_reachable(&(tuple, commit_id));
            } else {
                // No longer reachable - remove from our mapping
                self.t_to_commit_id.remove(&tuple);
            }
        }
    }

    fn step(&mut self, _recording: bool) -> bool {
        let mut changes = Vec::new();
        self.input.foreach(&mut |tuple: T, diff: Diff| {
            changes.push((tuple, diff));
        });

        if changes.is_empty() {
            return false;
        }

        // Increment commit ID for this step
        let current_id = self.commit_id.get();
        let new_id = CommitId::new(current_id.raw() + 1);
        self.commit_id.set(new_id);

        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in changes {
            // Track the T -> CommitId mapping (first discovery wins)
            let commit_id = *self.t_to_commit_id.entry(tuple.clone()).or_insert(new_id);
            // Also track input totals by T
            *self.input_totals_by_t.entry(tuple.clone()).or_insert(0) += diff.0;
            // Add to variable with commit ID stamp (use the mapped commit_id, not new_id)
            var.add_change((tuple, commit_id), diff);
        }
        var.commit();

        true
    }
}
