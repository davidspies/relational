//! FeedbackWithId wrapper - stamps tuples with CommitId when first seen.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
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
    /// Track input totals by T alone (not (T, CommitId)).
    input_totals: AHashMap<T, i64>,
    /// The seen set - tuples (by T) we've emitted +1 for.
    output_seen: HashSet<T>,
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
            output_seen: HashSet::new(),
            change_scratch: Multiset::new(),
            checkpoint_scratch: AHashMap::new(),
        }
    }

    pub(crate) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.variable.borrow_mut().push_checkpoint();
        }
    }

    /// Try to emit a tuple if it's reachable and not already seen.
    fn try_emit(
        input_totals: &AHashMap<T, i64>,
        output_seen: &mut HashSet<T>,
        var: &mut Variable<(T, CommitId)>,
        tuple: &T,
        commit_id: CommitId,
    ) {
        let input_total = input_totals.get(tuple).copied().unwrap_or(0);
        if input_total != 0 && !output_seen.contains(tuple) {
            output_seen.insert(tuple.clone());
            var.emit((tuple.clone(), commit_id));
        }
    }
}

impl<T: Clone + Eq + Hash, R: Op<T>> AnyFeedback for FeedbackWithIdWrapper<T, R> {
    fn push_checkpoint(&mut self) {
        self.variable.borrow_mut().push_checkpoint();
    }

    fn send_inverse(&mut self) {
        let mut var = self.variable.borrow_mut();
        // Collect from last checkpoint (don't pop yet)
        let tuples: Vec<_> = var.last_checkpoint().cloned().collect();
        for (tuple, commit_id) in &tuples {
            self.output_seen.remove(tuple);
            var.emit_inverse(&(tuple.clone(), *commit_id));
        }
    }

    fn commit(&mut self) {
        self.variable.borrow_mut().commit();
    }

    fn pop_pull_and_forward(&mut self) {
        // 1. Pop checkpoint and get its contents
        assert!(self.checkpoint_scratch.is_empty());
        for (tuple, commit_id) in self.variable.borrow_mut().pop_checkpoint_drain() {
            self.output_seen.remove(&tuple);
            self.checkpoint_scratch.insert(tuple, commit_id);
        }

        // 2. Pull changes and update input totals
        self.input.dump_to_multiset(&mut self.change_scratch);

        let current_id = self.commit_id.get();
        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in self.change_scratch.drain() {
            *self.input_totals.entry(tuple.clone()).or_insert(0) += diff;

            // Forward non-checkpoint items with current commit ID
            if !self.checkpoint_scratch.contains_key(&tuple) {
                Self::try_emit(
                    &self.input_totals,
                    &mut self.output_seen,
                    &mut var,
                    &tuple,
                    current_id,
                );
            }
        }

        // 3. Forward items from popped checkpoint that are still reachable
        // (keep their original commit_id)
        for (tuple, commit_id) in self.checkpoint_scratch.drain() {
            Self::try_emit(
                &self.input_totals,
                &mut self.output_seen,
                &mut var,
                &tuple,
                commit_id,
            );
        }
    }

    fn step(&mut self) -> bool {
        // Consolidate changes per tuple using Multiset to handle cases where
        // upstream emits both +1 and -1 for the same tuple within a single step.
        self.input.dump_to_multiset(&mut self.change_scratch);

        if self.change_scratch.is_empty() {
            return false;
        }

        let current_id = self.commit_id.get();
        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in self.change_scratch.drain() {
            *self.input_totals.entry(tuple.clone()).or_insert(0) += diff;
            Self::try_emit(
                &self.input_totals,
                &mut self.output_seen,
                &mut var,
                &tuple,
                current_id,
            );
        }
        var.commit();

        true
    }
}
