//! Feedback wrapper for type-erased feedback operations.

use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use crate::change::Diff;
use crate::collection::Multiset;
use crate::database::feedback::Variable;
use crate::database::relational::Op;

/// Type-erased feedback operations.
pub(crate) trait AnyFeedback {
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
pub(crate) struct FeedbackWrapper<T, R: Op<T>> {
    /// Shared variable state (also accessed by VariableRelation).
    variable: Rc<RefCell<Variable<T>>>,
    /// The input relation that feeds into this variable.
    input: R,
}

impl<T: Clone + Eq + Hash, R: Op<T>> FeedbackWrapper<T, R> {
    pub(crate) fn new(variable: Rc<RefCell<Variable<T>>>, input: R) -> Self {
        FeedbackWrapper { variable, input }
    }

    pub(crate) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.variable.borrow_mut().push_checkpoint();
        }
    }
}

impl<T: Clone + Eq + Hash, R: Op<T>> AnyFeedback for FeedbackWrapper<T, R> {
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
        let mut changes = Multiset::new();
        self.input.foreach(|tuple: T, diff: Diff| {
            changes.update(tuple, diff);
        });

        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in changes {
            var.update_input_total(tuple.clone(), diff);
            var.forward_if_not_in_checkpoint(&tuple);
        }
    }

    fn pop_and_forward_reachable(&mut self) {
        let mut variable = self.variable.borrow_mut();
        variable.pop_and_forward_reachable();
    }

    fn step(&mut self, _recording: bool) -> bool {
        // Consolidate changes per tuple using Multiset to handle cases where
        // upstream emits both +1 and -1 for the same tuple within a single step.
        // Without consolidation, the Variable's seen-set semantics would incorrectly
        // add tuples that net to zero.
        let mut changes = Multiset::new();
        self.input.foreach(|tuple: T, diff: Diff| {
            changes.update(tuple, diff);
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
