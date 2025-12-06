//! FeedbackWithId wrapper - stamps tuples with CommitId when first seen.

use std::cell::{Cell, RefCell};
use std::hash::Hash;
use std::rc::Rc;

use ahash::AHashMap;
use contiguous_data::{L2Vec, Multiset};

use crate::database::Relation;
use crate::database::commit_id::CommitId;
use crate::database::feedback::Variable;
use crate::database::relational::Op;

use super::feedback::AnyFeedback;

/// Wrapper for feedback_with_id - stamps tuples with CommitId when first seen.
/// Input relation produces T, variable stores (T, CommitId).
///
/// Uses `input_totals` for two purposes:
/// 1. Track cumulative input counts (non-zero = tuple is present in input)
/// 2. Track "has been emitted" (key exists = we've emitted +1 for this tuple)
pub(crate) struct FeedbackWithIdWrapper<T, R: Op<T>> {
    /// Shared variable state (also accessed by VariableRelation).
    variable: Rc<RefCell<Variable<(T, CommitId)>>>,
    /// Shared commit ID counter.
    commit_id: Rc<Cell<CommitId>>,
    /// The input relation produces T.
    input: Relation<R>,
    /// Cumulative input counts. Key exists = has been emitted.
    input_totals: AHashMap<T, i64>,
    /// Stack of outputs added at each checkpoint level (T -> CommitId).
    outputs_by_checkpoint: L2Vec<(T, CommitId)>,
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
            input_totals: AHashMap::new(),
            outputs_by_checkpoint: L2Vec::new(),
            change_scratch: Multiset::new(),
            checkpoint_scratch: AHashMap::new(),
        }
    }

    pub(crate) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.outputs_by_checkpoint.push_empty();
        }
    }

}

impl<T: Clone + Eq + Hash, R: Op<T>> AnyFeedback for FeedbackWithIdWrapper<T, R> {
    fn push_checkpoint(&mut self) {
        self.outputs_by_checkpoint.push_empty();
    }

    fn send_inverse(&mut self) {
        // Collect from last checkpoint (don't pop yet)
        let tuples: Vec<_> = self
            .outputs_by_checkpoint
            .last()
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        let mut var = self.variable.borrow_mut();
        for (tuple, commit_id) in &tuples {
            var.emit_inverse(&(tuple.clone(), *commit_id));
        }
    }

    fn commit(&mut self) {
        self.variable.borrow_mut().commit();
    }

    fn pop_pull_and_forward(&mut self) {
        // 1. Pop checkpoint and collect its contents
        assert!(self.checkpoint_scratch.is_empty());
        for (tuple, commit_id) in self.outputs_by_checkpoint.pop().into_iter().flatten() {
            self.checkpoint_scratch.insert(tuple, commit_id);
        }

        // 2. Pull changes and update input totals, emit as needed
        self.input.dump_to_multiset(&mut self.change_scratch);

        let current_id = self.commit_id.get();
        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in self.change_scratch.drain() {
            let was_emitted = self.input_totals.contains_key(&tuple);
            let checkpoint_id = self.checkpoint_scratch.remove(&tuple);
            *self.input_totals.entry(tuple.clone()).or_insert(0) += diff;
            let total = *self.input_totals.get(&tuple).unwrap();

            // Emit if:
            // - Checkpoint tuple that's still present (re-emit with original id)
            // - OR new tuple that's now present (emit with current id)
            if total != 0 {
                if let Some(id) = checkpoint_id {
                    // Checkpoint tuple - re-emit with original commit_id
                    var.emit((tuple.clone(), id));
                    if !self.outputs_by_checkpoint.is_empty() {
                        self.outputs_by_checkpoint.push((tuple, id));
                    }
                } else if !was_emitted {
                    // New tuple appearing for first time
                    var.emit((tuple.clone(), current_id));
                    if !self.outputs_by_checkpoint.is_empty() {
                        self.outputs_by_checkpoint.push((tuple, current_id));
                    }
                }
            } else {
                // Tuple is gone, remove from input_totals
                self.input_totals.remove(&tuple);
            }
        }
        drop(var);

        // 3. Re-emit remaining checkpoint tuples (not in change_scratch) if still present
        let mut var = self.variable.borrow_mut();
        for (tuple, commit_id) in self.checkpoint_scratch.drain() {
            let total = self.input_totals.get(&tuple).copied().unwrap_or(0);
            if total != 0 {
                var.emit((tuple.clone(), commit_id));
                if !self.outputs_by_checkpoint.is_empty() {
                    self.outputs_by_checkpoint.push((tuple, commit_id));
                }
            } else {
                // Checkpoint tuple is now gone, remove from input_totals
                self.input_totals.remove(&tuple);
            }
        }
    }

    fn step(&mut self) -> bool {
        // Consolidate changes per tuple using Multiset to handle cases where
        // upstream emits both +1 and -1 for the same tuple within a single step.
        self.input.dump_to_multiset(&mut self.change_scratch);

        if self.change_scratch.is_empty() {
            return false;
        }

        // Collect tuples to emit (can't modify outputs_by_checkpoint while iterating)
        let current_id = self.commit_id.get();
        let mut to_emit = Vec::new();
        for (tuple, diff) in self.change_scratch.drain() {
            let was_emitted = self.input_totals.contains_key(&tuple);
            *self.input_totals.entry(tuple.clone()).or_insert(0) += diff;
            let total = *self.input_totals.get(&tuple).unwrap();

            // Emit if present (count != 0) AND not previously emitted
            if total != 0 && !was_emitted {
                to_emit.push(tuple);
            }
        }

        let mut var = self.variable.borrow_mut();
        for tuple in to_emit {
            var.emit((tuple.clone(), current_id));
            if !self.outputs_by_checkpoint.is_empty() {
                self.outputs_by_checkpoint.push((tuple, current_id));
            }
        }
        var.commit();

        true
    }
}
