//! Feedback wrapper for type-erased feedback operations.

use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use contiguous_data::Multiset;

use crate::database::Relation;
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
    /// Pop checkpoint, pull changes, and forward reachable items.
    /// Combines the old pull_and_forward_non_checkpoint + pop_and_forward_reachable,
    /// but pops first so all mutations go to the new last checkpoint.
    fn pop_pull_and_forward(&mut self);
    /// Run one step: pull from input relation, add to variable. Returns true if new output.
    fn step(&mut self, recording: bool) -> bool;
}

/// Wrapper to make feedback type-erased.
pub(crate) struct FeedbackWrapper<T, R: Op<T>> {
    /// Shared variable state (also accessed by VariableRelation).
    variable: Rc<RefCell<Variable<T>>>,
    /// The input relation that feeds into this variable.
    input: Relation<R>,
}

impl<T: Clone + Eq + Hash, R: Op<T>> FeedbackWrapper<T, R> {
    pub(crate) fn new(variable: Rc<RefCell<Variable<T>>>, input: Relation<R>) -> Self {
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

    fn pop_pull_and_forward(&mut self) {
        // 1. Pop checkpoint and get its contents
        let checkpoint_tuples: Vec<T> = {
            let mut var = self.variable.borrow_mut();
            var.pop_checkpoint_drain().collect()
        };

        // 2. Pull changes and update input totals
        let mut changes = Multiset::new();
        self.input.dump_to_multiset(&mut changes);

        let change_tuples: Vec<_> = changes.drain().collect();

        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in &change_tuples {
            var.update_input_total(tuple.clone(), *diff);
        }

        // 3. Forward items from changes that weren't in the popped checkpoint
        for (tuple, _) in change_tuples {
            if !checkpoint_tuples.contains(&tuple) {
                var.forward_reachable(&tuple);
            }
        }

        // 4. Forward items from popped checkpoint that are still reachable
        for tuple in checkpoint_tuples {
            var.forward_reachable(&tuple);
        }
    }

    fn step(&mut self, _recording: bool) -> bool {
        // Consolidate changes per tuple using Multiset to handle cases where
        // upstream emits both +1 and -1 for the same tuple within a single step.
        // Without consolidation, the Variable's seen-set semantics would incorrectly
        // add tuples that net to zero.
        let mut changes = Multiset::new();
        self.input.dump_to_multiset(&mut changes);

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
