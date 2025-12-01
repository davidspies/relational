//! FeedbackWithId wrapper - stamps tuples with CommitId when first seen.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use crate::collection::Multiset;
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
    input: R,
    /// Track input totals by T alone (not (T, CommitId)) for pop() handling.
    input_totals_by_t: HashMap<T, i64>,
    /// Maps T -> CommitId for tuples currently in output.
    t_to_commit_id: HashMap<T, CommitId>,
}

impl<T: Clone + Eq + Hash, R: Op<T>> FeedbackWithIdWrapper<T, R> {
    pub(crate) fn new(
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

    fn pull_and_forward_non_checkpoint(&mut self) {
        let mut changes = Multiset::new();
        self.input.dump_to_multiset(&mut changes);

        // Update input_totals and forward in a single pass
        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in changes {
            // Update our T-keyed input_totals
            let input_total = self.input_totals_by_t.entry(tuple.clone()).or_insert(0);
            *input_total += diff;

            // Look up the actual (T, CommitId) in our mapping and forward if not in checkpoint
            if let Some(&commit_id) = self.t_to_commit_id.get(&tuple) {
                let full_tuple = (tuple, commit_id);
                // We need to sync the variable's view - set it to match our tracking
                var.set_input_total(full_tuple.clone(), *input_total);
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
            // Track the T -> CommitId mapping (first discovery wins)
            let commit_id = *self
                .t_to_commit_id
                .entry(tuple.clone())
                .or_insert(current_id);
            // Also track input totals by T
            *self.input_totals_by_t.entry(tuple.clone()).or_insert(0) += diff;
            // Add to variable with commit ID stamp (use the mapped commit_id, not new_id)
            var.add_change((tuple, commit_id), diff);
        }
        var.commit();

        true
    }
}
