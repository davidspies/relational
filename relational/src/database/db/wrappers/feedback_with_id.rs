//! FeedbackWithId wrapper - stamps tuples with CommitId when first seen.

use std::cell::{Cell, RefCell};
use std::hash::Hash;
use std::rc::Rc;

use ahash::AHashMap;
use contiguous_data::Multiset;

use crate::database::Relation;
use crate::database::commit_id::CommitId;
use crate::database::feedback::Variable;
use crate::database::relational::Op;

use super::feedback::AnyFeedback;

/// Wrapper for feedback_with_id - stamps tuples with CommitId when first seen.
/// Input relation produces T, variable stores (T, CommitId).
pub(crate) struct FeedbackWithIdWrapper<T, R: Op<T>> {
    /// Shared variable state (also accessed by VariableRelation).
    variable: Rc<RefCell<Variable<(T, CommitId)>>>,
    /// Shared commit ID counter.
    commit_id: Rc<Cell<CommitId>>,
    /// The input relation produces T.
    input: Relation<R>,
    /// Track input totals by T alone (not (T, CommitId)) for pop() handling.
    input_totals_by_t: AHashMap<T, i64>,
    /// Scratch space for collecting changes.
    change_scratch: Multiset<T>,
    /// Scratch space for checkpoint tuples during pop (T -> CommitId).
    checkpoint_scratch: AHashMap<T, CommitId>,
}

impl<T: Clone + Eq + Hash, R: Op<T>> FeedbackWithIdWrapper<T, R> {
    pub(crate) fn new(
        variable: Rc<RefCell<Variable<(T, CommitId)>>>,
        input: Relation<R>,
        commit_id: Rc<Cell<CommitId>>,
    ) -> Self {
        FeedbackWithIdWrapper {
            variable,
            commit_id,
            input,
            input_totals_by_t: AHashMap::new(),
            change_scratch: Multiset::new(),
            checkpoint_scratch: AHashMap::new(),
        }
    }

    pub(crate) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.variable.borrow_mut().push_checkpoint();
        }
    }
}

impl<T: Clone + Eq + Hash, R: Op<T>> AnyFeedback for FeedbackWithIdWrapper<T, R> {
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
        assert!(self.checkpoint_scratch.is_empty());
        for (tuple, commit_id) in self.variable.borrow_mut().pop_checkpoint_drain() {
            self.checkpoint_scratch.insert(tuple, commit_id);
        }

        // 2. Pull changes, update input totals, and forward non-checkpoint items
        self.input.dump_to_multiset(&mut self.change_scratch);

        let current_id = self.commit_id.get();
        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in self.change_scratch.drain() {
            *self.input_totals_by_t.entry(tuple.clone()).or_insert(0) += diff;

            if !self.checkpoint_scratch.contains_key(&tuple) {
                let full_tuple = (tuple.clone(), current_id);
                let input_total = self.input_totals_by_t.get(&tuple).copied().unwrap_or(0);
                var.set_input_total(full_tuple.clone(), input_total);
                var.forward_reachable(&full_tuple);
            }
        }

        // 3. Forward items from popped checkpoint that are still reachable
        for (tuple, commit_id) in self.checkpoint_scratch.drain() {
            let input_total = self.input_totals_by_t.get(&tuple).copied().unwrap_or(0);
            if input_total != 0 {
                var.forward_reachable(&(tuple, commit_id));
            }
        }
    }

    fn step(&mut self) -> bool {
        // Consolidate changes per tuple using Multiset to handle cases where
        // upstream emits both +1 and -1 for the same tuple within a single step.
        // Without consolidation, the Variable's seen-set semantics would incorrectly
        // add tuples that net to zero.
        let mut changes = Multiset::new();
        self.input.dump_to_multiset(&mut changes);

        if changes.is_empty() {
            return false;
        }

        let current_id = self.commit_id.get();

        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in changes {
            // Also track input totals by T
            *self.input_totals_by_t.entry(tuple.clone()).or_insert(0) += diff;
            // Add to variable with commit ID stamp (use the mapped commit_id, not new_id)
            var.add_input((tuple, current_id), diff);
        }
        var.commit();

        true
    }
}
