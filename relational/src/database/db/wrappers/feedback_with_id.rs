//! FeedbackWithId wrapper - stamps tuples with CommitId when first seen.

use std::cell::{Cell, RefCell};
use std::collections::hash_map;
use std::hash::Hash;
use std::rc::Rc;

use ahash::{AHashMap, AHashSet};
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
    /// Scratch space for checkpoint tuples during pop.
    checkpoint_scratch: AHashSet<T>,
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
            checkpoint_scratch: AHashSet::new(),
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
        let tuples = self.outputs_by_checkpoint.last().unwrap();
        let mut var = self.variable.borrow_mut();
        for (tuple, commit_id) in tuples {
            var.emit_inverse(&(tuple.clone(), *commit_id));
        }
    }

    fn commit(&mut self) {
        self.variable.borrow_mut().commit();
    }

    fn pop_pull_and_forward(&mut self) {
        assert!(self.checkpoint_scratch.is_empty());
        self.checkpoint_scratch
            .extend(self.outputs_by_checkpoint.pop().unwrap().map(|(t, _)| t));
        self.input.dump_to_multiset(&mut self.change_scratch);

        let current_id = self.commit_id.get();
        let mut var = self.variable.borrow_mut();

        for tuple in self.checkpoint_scratch.drain() {
            let diff = self.change_scratch.remove(&tuple);
            let input_total = self.input_totals.get_mut(&tuple).unwrap();
            *input_total += diff;
            if *input_total == 0 {
                self.input_totals.remove(&tuple);
            } else {
                if !self.outputs_by_checkpoint.is_empty() {
                    self.outputs_by_checkpoint.push((tuple.clone(), current_id));
                }
                var.emit((tuple, current_id));
            }
        }

        for (tuple, diff) in self.change_scratch.drain() {
            match self.input_totals.entry(tuple) {
                hash_map::Entry::Occupied(occupied_entry) => *occupied_entry.into_mut() += diff,
                hash_map::Entry::Vacant(vacant_entry) => {
                    let tuple = vacant_entry.key();
                    if !self.outputs_by_checkpoint.is_empty() {
                        self.outputs_by_checkpoint.push((tuple.clone(), current_id));
                    }
                    var.emit((tuple.clone(), current_id));
                    vacant_entry.insert(diff);
                }
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
