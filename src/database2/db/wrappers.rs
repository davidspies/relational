//! Type-erased wrappers for database-managed components.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use crate::change::Diff;
use crate::database2::commit_id::CommitId;
use crate::database2::feedback::Variable;
use crate::database2::relational::input::InputState;
use crate::database2::relational::Relation;
use crate::Tuple;

/// Type-erased input handle operations.
pub(super) trait AnyInput {
    /// Push a checkpoint - start recording inserts.
    fn push_checkpoint(&self);
    /// Record any new inserts (from pending) to the current checkpoint.
    /// Called during db.commit() to capture what was inserted.
    fn record_pending_inserts(&self);
    /// Remove all tuples inserted in the current checkpoint and pop.
    /// Used as step 1 of the database pop() algorithm.
    fn send_inverse_and_pop(&self);
}

/// Wrapper to make InputHandle type-erased (seen-set semantics).
pub(super) struct InputWrapper<T: Tuple> {
    pub(super) state: Rc<RefCell<InputState<T>>>,
    /// Stack of tuples inserted at each checkpoint level.
    checkpoint_stack: RefCell<Vec<HashSet<T>>>,
}

impl<T: Tuple> InputWrapper<T> {
    pub(super) fn new(state: Rc<RefCell<InputState<T>>>) -> Self {
        InputWrapper {
            state,
            checkpoint_stack: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn push_initial_checkpoints(&self, depth: usize) {
        for _ in 0..depth {
            self.checkpoint_stack.borrow_mut().push(HashSet::new());
        }
    }
}

impl<T: Tuple + 'static> AnyInput for InputWrapper<T> {
    fn push_checkpoint(&self) {
        self.checkpoint_stack.borrow_mut().push(HashSet::new());
    }

    fn record_pending_inserts(&self) {
        if let Some(level) = self.checkpoint_stack.borrow_mut().last_mut() {
            // Take newly inserted tuples and record them to the checkpoint
            let new_inserts = self.state.borrow_mut().take_new_inserts();
            level.extend(new_inserts);
        } else {
            // Not recording, just clear the new_inserts buffer
            self.state.borrow_mut().take_new_inserts();
        }
    }

    fn send_inverse_and_pop(&self) {
        if let Some(tuples) = self.checkpoint_stack.borrow_mut().pop() {
            let mut state = self.state.borrow_mut();
            // Remove each tuple that was inserted in this checkpoint
            for tuple in tuples {
                state.remove(&tuple);
            }
        }
    }
}

/// Type-erased feedback operations.
pub(super) trait AnyFeedback {
    /// Push a checkpoint.
    fn push_checkpoint(&self);
    /// Send -1 for all outputs in the current checkpoint (don't pop yet).
    /// Used as step 1 of the database pop() algorithm.
    fn send_inverse(&self);
    /// Commit the variable's current changes so they can be pulled by downstream.
    /// Used as step 2 of the database pop() algorithm.
    fn commit(&self);
    /// Pull changes, update tracked inputs, forward +1 for items not in last checkpoint.
    /// Used as step 3a of the database pop() algorithm.
    fn pull_and_forward_non_checkpoint(&self);
    /// Pop checkpoint, forward +1 for items whose tracked input is still positive.
    /// Used as step 3b of the database pop() algorithm.
    fn pop_and_forward_reachable(&self);
    /// Run one step: pull from input relation, add to variable. Returns true if new output.
    fn step(&self, recording: bool) -> bool;
}

/// Wrapper to make feedback type-erased.
pub(super) struct FeedbackWrapper<T: Tuple, R: Relation<T>> {
    pub(super) variable: Rc<RefCell<Variable<T>>>,
    /// The input relation that feeds into this variable.
    input: RefCell<R>,
}

impl<T: Tuple, R: Relation<T>> FeedbackWrapper<T, R> {
    pub(super) fn new(variable: Rc<RefCell<Variable<T>>>, input: R) -> Self {
        FeedbackWrapper {
            variable,
            input: RefCell::new(input),
        }
    }

    pub(super) fn push_initial_checkpoints(&self, depth: usize) {
        for _ in 0..depth {
            self.variable.borrow_mut().push_checkpoint();
        }
    }
}

impl<T: Tuple + 'static, R: Relation<T> + 'static> AnyFeedback for FeedbackWrapper<T, R> {
    fn push_checkpoint(&self) {
        self.variable.borrow_mut().push_checkpoint();
    }

    fn send_inverse(&self) {
        self.variable.borrow_mut().send_inverse();
    }

    fn commit(&self) {
        self.variable.borrow_mut().commit();
    }

    fn pull_and_forward_non_checkpoint(&self) {
        // Collect changes from input relation
        let mut changes = Vec::new();
        self.input.borrow_mut().foreach(&mut |tuple: T, diff: Diff| {
            changes.push((tuple, diff));
        });

        // Update input_totals and forward non-checkpoint items
        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in changes {
            var.update_input_total(tuple.clone(), diff);
            var.forward_if_not_in_checkpoint(&tuple);
        }
    }

    fn pop_and_forward_reachable(&self) {
        self.variable.borrow_mut().pop_and_forward_reachable();
    }

    fn step(&self, _recording: bool) -> bool {
        // Collect first to avoid borrow conflicts
        let mut changes = Vec::new();
        self.input.borrow_mut().foreach(&mut |tuple: T, diff: Diff| {
            changes.push((tuple, diff));
        });

        if changes.is_empty() {
            return false;
        }

        // Apply all changes to variable
        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in changes {
            var.add_change(tuple, diff);
        }

        // Commit staged to pending so they can be pulled
        var.commit();

        true
    }
}

/// Type-erased interrupt operations.
pub(super) trait AnyInterrupt {
    /// Check if the interrupt condition is met (relation has positive entries).
    /// Returns true if the fixpoint should stop.
    fn check(&mut self) -> bool;
}

/// Wrapper for interrupt - checks if a relation has any positive entries.
pub(super) struct InterruptWrapper<T: Tuple, R: Relation<T>> {
    input: R,
    /// Track if we've seen any positive entries.
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
        // Pull changes and check if any have positive diff
        self.input.foreach(&mut |_, diff| {
            if diff.0 > 0 {
                self.has_positive = true;
            }
        });
        self.has_positive
    }
}

/// Wrapper for feedback_with_id - stamps tuples with CommitId when first seen.
/// Input relation produces T, variable stores (T, CommitId).
pub(super) struct FeedbackWithIdWrapper<T: Tuple, R: Relation<T>> {
    /// The variable stores (T, CommitId).
    pub(super) variable: Rc<RefCell<Variable<(T, CommitId)>>>,
    /// The input relation produces T.
    input: RefCell<R>,
    /// Shared commit ID counter.
    commit_id: Rc<Cell<CommitId>>,
}

impl<T: Tuple, R: Relation<T>> FeedbackWithIdWrapper<T, R> {
    pub(super) fn new(
        variable: Rc<RefCell<Variable<(T, CommitId)>>>,
        input: R,
        commit_id: Rc<Cell<CommitId>>,
    ) -> Self {
        FeedbackWithIdWrapper {
            variable,
            input: RefCell::new(input),
            commit_id,
        }
    }

    pub(super) fn push_initial_checkpoints(&self, depth: usize) {
        for _ in 0..depth {
            self.variable.borrow_mut().push_checkpoint();
        }
    }
}

impl<T: Tuple + 'static, R: Relation<T> + 'static> AnyFeedback for FeedbackWithIdWrapper<T, R> {
    fn push_checkpoint(&self) {
        self.variable.borrow_mut().push_checkpoint();
    }

    fn send_inverse(&self) {
        self.variable.borrow_mut().send_inverse();
    }

    fn commit(&self) {
        self.variable.borrow_mut().commit();
    }

    fn pull_and_forward_non_checkpoint(&self) {
        // Collect changes from input relation
        let mut changes = Vec::new();
        self.input.borrow_mut().foreach(&mut |tuple: T, diff: Diff| {
            changes.push((tuple, diff));
        });

        // For feedback_with_id, input is T but variable stores (T, CommitId)
        // We need to handle this specially - input_totals are keyed by T
        // but outputs are (T, CommitId). For now, use a dummy CommitId.
        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in changes {
            var.update_input_total((tuple.clone(), CommitId::new(0)), diff);
            var.forward_if_not_in_checkpoint(&(tuple, CommitId::new(0)));
        }
    }

    fn pop_and_forward_reachable(&self) {
        self.variable.borrow_mut().pop_and_forward_reachable();
    }

    fn step(&self, _recording: bool) -> bool {
        // Collect first to avoid borrow conflicts
        let mut changes = Vec::new();
        self.input.borrow_mut().foreach(&mut |tuple: T, diff: Diff| {
            changes.push((tuple, diff));
        });

        if changes.is_empty() {
            return false;
        }

        // Increment commit ID for this step
        let current_id = self.commit_id.get();
        let new_id = CommitId::new(current_id.raw() + 1);
        self.commit_id.set(new_id);

        // Apply to variable - stamp each tuple with the current commit ID
        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in changes {
            // Stamp with current commit ID
            var.add_change((tuple, new_id), diff);
        }

        // Commit staged to pending so they can be pulled
        var.commit();

        true
    }
}
