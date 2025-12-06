//! Feedback wrapper for type-erased feedback operations.

use std::cell::RefCell;
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

use ahash::AHashMap;
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
    fn step(&mut self) -> bool;
}

/// Wrapper to make feedback type-erased.
pub(crate) struct FeedbackWrapper<T, R: Op<T>> {
    /// Shared variable state (also accessed by VariableRelation).
    variable: Rc<RefCell<Variable<T>>>,
    /// The input relation that feeds into this variable.
    input: Relation<R>,
    /// Cumulative input multiplicities. A tuple is "reachable" when non-zero.
    input_totals: AHashMap<T, i64>,
    /// The seen set - tuples we've emitted +1 for.
    output_seen: HashSet<T>,
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
            output_seen: HashSet::new(),
            change_scratch: Multiset::new(),
            checkpoint_scratch: HashSet::new(),
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
        var: &mut Variable<T>,
        tuple: &T,
    ) {
        let input_total = input_totals.get(tuple).copied().unwrap_or(0);
        if input_total != 0 && !output_seen.contains(tuple) {
            output_seen.insert(tuple.clone());
            var.emit(tuple.clone());
        }
    }
}

impl<T: Clone + Eq + Hash, R: Op<T>> AnyFeedback for FeedbackWrapper<T, R> {
    fn push_checkpoint(&mut self) {
        self.variable.borrow_mut().push_checkpoint();
    }

    fn send_inverse(&mut self) {
        let mut var = self.variable.borrow_mut();
        // Collect from last checkpoint (don't pop yet)
        let tuples: Vec<_> = var.last_checkpoint().cloned().collect();
        for tuple in &tuples {
            self.output_seen.remove(tuple);
            var.emit_inverse(tuple);
        }
    }

    fn commit(&mut self) {
        self.variable.borrow_mut().commit();
    }

    fn pop_pull_and_forward(&mut self) {
        // 1. Pop checkpoint and get its contents (tuples to potentially re-forward)
        assert!(self.checkpoint_scratch.is_empty());
        self.checkpoint_scratch
            .extend(self.variable.borrow_mut().pop_checkpoint_drain());

        // 2. Pull changes and update input totals
        self.input.dump_to_multiset(&mut self.change_scratch);

        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in self.change_scratch.drain() {
            *self.input_totals.entry(tuple.clone()).or_insert(0) += diff;

            // Forward non-checkpoint items if reachable
            if !self.checkpoint_scratch.contains(&tuple) {
                Self::try_emit(&self.input_totals, &mut self.output_seen, &mut var, &tuple);
            }
        }

        // 3. Forward items from popped checkpoint that are still reachable
        for tuple in self.checkpoint_scratch.drain() {
            Self::try_emit(&self.input_totals, &mut self.output_seen, &mut var, &tuple);
        }
    }

    fn step(&mut self) -> bool {
        // Consolidate changes per tuple using Multiset to handle cases where
        // upstream emits both +1 and -1 for the same tuple within a single step.
        self.input.dump_to_multiset(&mut self.change_scratch);

        if self.change_scratch.is_empty() {
            return false;
        }

        let mut var = self.variable.borrow_mut();
        for (tuple, diff) in self.change_scratch.drain() {
            *self.input_totals.entry(tuple.clone()).or_insert(0) += diff;
            Self::try_emit(&self.input_totals, &mut self.output_seen, &mut var, &tuple);
        }
        var.commit();

        true
    }
}
