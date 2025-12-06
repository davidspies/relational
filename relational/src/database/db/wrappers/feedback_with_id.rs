//! Generic feedback wrapper with optional CommitId stamping.

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

/// Marker: strip CommitId, output just T.
#[derive(Default)]
pub(crate) struct NotWithId;
/// Marker: keep CommitId, output (T, CommitId).
#[derive(Default)]
pub(crate) struct WithId;

/// Convert (T, CommitId) to output type V.
pub(crate) trait Convert<T, V>: Default {
    fn convert(&self, input: (T, CommitId)) -> V;
}

impl<T> Convert<T, T> for NotWithId {
    fn convert(&self, (input, _): (T, CommitId)) -> T {
        input
    }
}

impl<T> Convert<T, (T, CommitId)> for WithId {
    fn convert(&self, input: (T, CommitId)) -> (T, CommitId) {
        input
    }
}

/// Generic feedback wrapper.
///
/// - T: input tuple type
/// - R: input relation operator
/// - V: variable output type (T or (T, CommitId))
/// - C: converter from (T, CommitId) to V
pub(crate) struct FeedbackWrapperG<T, R: Op<T>, V, C: Convert<T, V>> {
    variable: Rc<RefCell<Variable<V>>>,
    commit_id: Rc<Cell<CommitId>>,
    input: Relation<R>,
    input_totals: AHashMap<T, i64>,
    outputs_by_checkpoint: L2Vec<(T, CommitId)>,
    change_scratch: Multiset<T>,
    checkpoint_scratch: AHashSet<T>,
    converter: C,
}

/// Regular feedback: variable stores T.
pub(crate) type FeedbackWrapper<T, R> = FeedbackWrapperG<T, R, T, NotWithId>;
/// Feedback with ID: variable stores (T, CommitId).
pub(crate) type FeedbackWithIdWrapper<T, R> = FeedbackWrapperG<T, R, (T, CommitId), WithId>;

impl<T: Clone + Eq + Hash, R: Op<T>, V: Clone + Eq + Hash, C: Convert<T, V>>
    FeedbackWrapperG<T, R, V, C>
{
    pub(crate) fn new(
        variable: Rc<RefCell<Variable<V>>>,
        input: Relation<R>,
        commit_id: Rc<Cell<CommitId>>,
    ) -> Self {
        FeedbackWrapperG {
            variable,
            commit_id,
            input,
            input_totals: AHashMap::new(),
            outputs_by_checkpoint: L2Vec::new(),
            change_scratch: Multiset::new(),
            checkpoint_scratch: AHashSet::new(),
            converter: C::default(),
        }
    }

    pub(crate) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.outputs_by_checkpoint.push_empty();
        }
    }
}

impl<T: Clone + Eq + Hash, R: Op<T>, V: Clone + Eq + Hash, C: Convert<T, V>> AnyFeedback
    for FeedbackWrapperG<T, R, V, C>
{
    fn push_checkpoint(&mut self) {
        self.outputs_by_checkpoint.push_empty();
    }

    fn send_inverse(&mut self) {
        let tuples = self.outputs_by_checkpoint.last().unwrap();
        let mut var = self.variable.borrow_mut();
        for (tuple, commit_id) in tuples {
            var.emit_inverse(&self.converter.convert((tuple.clone(), *commit_id)));
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
                var.emit(self.converter.convert((tuple, current_id)));
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
                    var.emit(self.converter.convert((tuple.clone(), current_id)));
                    vacant_entry.insert(diff);
                }
            }
        }
    }

    fn step(&mut self) -> bool {
        self.input.dump_to_multiset(&mut self.change_scratch);

        if self.change_scratch.is_empty() {
            return false;
        }

        let current_id = self.commit_id.get();
        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in self.change_scratch.drain() {
            let was_emitted = self.input_totals.contains_key(&tuple);
            *self.input_totals.entry(tuple.clone()).or_insert(0) += diff;
            let total = *self.input_totals.get(&tuple).unwrap();

            if total != 0 && !was_emitted {
                var.emit(self.converter.convert((tuple.clone(), current_id)));
                if !self.outputs_by_checkpoint.is_empty() {
                    self.outputs_by_checkpoint.push((tuple, current_id));
                }
            }
        }
        var.commit();

        true
    }
}
