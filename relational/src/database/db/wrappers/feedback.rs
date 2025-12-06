//! Feedback wrapper for type-erased feedback operations.

use std::cell::RefCell;
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

use ahash::AHashMap;
use contiguous_data::{L2Vec, Multiset};

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
    fn step(&mut self) -> bool;
}

/// Wrapper to make feedback type-erased.
///
/// Uses `input_totals` for two purposes:
/// 1. Track cumulative input counts (non-zero = tuple is present in input)
/// 2. Track "has been emitted" (key exists = we've emitted +1 for this tuple)
///
/// A key with value 0 means "was emitted but input count is now zero".
pub(crate) struct FeedbackWrapper<T, R: Op<T>> {
    /// Shared variable state (also accessed by VariableRelation).
    variable: Rc<RefCell<Variable<T>>>,
    /// The input relation that feeds into this variable.
    input: Relation<R>,
    /// Cumulative input counts. Key exists = has been emitted.
    /// Value 0 = emitted but no longer present. Value != 0 = emitted and present.
    input_totals: AHashMap<T, i64>,
    /// Stack of outputs added at each checkpoint level.
    outputs_by_checkpoint: L2Vec<T>,
    /// Scratch space for collecting changes.
    change_scratch: Multiset<T>,
    /// Scratch space for checkpoint tuples during pop.
    checkpoint_scratch: HashSet<T>,
}

impl<T: Clone + Eq + Hash, R: Op<T>> FeedbackWrapper<T, R> {
    pub(crate) fn new(variable: Rc<RefCell<Variable<T>>>, input: Relation<R>) -> Self {
        FeedbackWrapper {
            variable,
            input,
            input_totals: AHashMap::new(),
            outputs_by_checkpoint: L2Vec::new(),
            change_scratch: Multiset::new(),
            checkpoint_scratch: HashSet::new(),
        }
    }

    pub(crate) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.outputs_by_checkpoint.push_empty();
        }
    }

}

impl<T: Clone + Eq + Hash, R: Op<T>> AnyFeedback for FeedbackWrapper<T, R> {
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
        for tuple in &tuples {
            var.emit_inverse(tuple);
        }
    }

    fn commit(&mut self) {
        self.variable.borrow_mut().commit();
    }

    fn pop_pull_and_forward(&mut self) {
        assert!(self.checkpoint_scratch.is_empty());
        for tuple in self.outputs_by_checkpoint.pop().into_iter().flatten() {
            self.checkpoint_scratch.insert(tuple);
        }
        self.input.dump_to_multiset(&mut self.change_scratch);

        let mut to_emit = Vec::new();

        for (tuple, diff) in self.change_scratch.drain() {
            let was_emitted = self.input_totals.contains_key(&tuple);
            let is_checkpoint = self.checkpoint_scratch.remove(&tuple);
            *self.input_totals.entry(tuple.clone()).or_insert(0) += diff;
            let total = self.input_totals[&tuple];

            if total != 0 && (is_checkpoint || !was_emitted) {
                to_emit.push(tuple);
            } else if total == 0 && is_checkpoint {
                self.input_totals.remove(&tuple);
            }
        }

        for tuple in self.checkpoint_scratch.drain() {
            if self.input_totals.get(&tuple).copied().unwrap_or(0) != 0 {
                to_emit.push(tuple);
            } else {
                self.input_totals.remove(&tuple);
            }
        }

        let mut var = self.variable.borrow_mut();
        for tuple in to_emit {
            var.emit(tuple.clone());
            if !self.outputs_by_checkpoint.is_empty() {
                self.outputs_by_checkpoint.push(tuple);
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

        // Collect tuples to emit (can't call emit_and_track while iterating)
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
            var.emit(tuple.clone());
            if !self.outputs_by_checkpoint.is_empty() {
                self.outputs_by_checkpoint.push(tuple);
            }
        }
        var.commit();

        true
    }
}
