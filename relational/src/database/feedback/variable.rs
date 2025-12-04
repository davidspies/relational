//! Variable for tracking iterative computation state.

use std::collections::HashSet;
use std::hash::Hash;

use ahash::AHashMap;
use contiguous_data::{Diff, L2Vec, Multiset};

/// A variable in an iterative computation.
///
/// Variables track a "seen set" - tuples are only emitted once when they
/// first become non-zero in input_totals. This ensures monotonic growth toward fixpoint.
///
/// The variable has two separate tracking mechanisms:
/// - `input_totals`: Tracks cumulative input multiplicities
/// - `output_seen`: The seen set - tuples we've emitted +1 for
///
/// During normal operation, tuples are added to output_seen when they first become
/// non-zero in input_totals. During pop(), we manipulate output_seen directly.
pub struct Variable<T> {
    /// Cumulative input multiplicities.
    /// A tuple is considered "reachable" when this is non-zero.
    input_totals: AHashMap<T, i64>,
    /// The seen set - tuples we've emitted +1 for.
    output_seen: HashSet<T>,
    /// Staged changes (not yet committed).
    staged: Multiset<T>,
    /// Pending changes (committed, ready to be pulled).
    pending: Multiset<T>,
    /// Stack of outputs added at each checkpoint level.
    outputs_by_checkpoint: L2Vec<T>,
}

impl<T: Clone + Eq + Hash> Variable<T> {
    /// Create a new empty variable.
    pub fn new() -> Self {
        Variable {
            input_totals: AHashMap::new(),
            output_seen: HashSet::new(),
            staged: Multiset::new(),
            pending: Multiset::new(),
            outputs_by_checkpoint: L2Vec::new(),
        }
    }

    /// Add input to this variable (used during normal fixpoint).
    /// Only emits +1 if the tuple is not already in output_seen AND input_totals is non-zero.
    /// Once a tuple is seen, it stays in output until explicitly removed via pop().
    pub(crate) fn add_input(&mut self, tuple: T, diff: Diff) {
        // Update input_totals
        let total = self.input_totals.entry(tuple.clone()).or_insert(0);
        *total += diff;

        // Only add to output if:
        // 1. input_totals is now non-zero, AND
        // 2. not already in output_seen (seen set semantics)
        if *total != 0 && !self.output_seen.contains(&tuple) {
            self.output_seen.insert(tuple.clone());
            self.staged.update(tuple.clone(), 1);
            if !self.outputs_by_checkpoint.is_empty() {
                self.outputs_by_checkpoint.push(tuple);
            }
        }
        // If input_totals == 0 or already in output_seen, do nothing
    }

    /// Backwards compatibility alias for add_input.
    pub(crate) fn add_change(&mut self, tuple: T, diff: Diff) {
        self.add_input(tuple, diff);
    }

    /// Take the pending changes (empties the buffer).
    pub(crate) fn drain_pending(&mut self) -> impl Iterator<Item = (T, Diff)> {
        self.pending.drain()
    }

    /// Commit staged changes to pending.
    pub(crate) fn commit(&mut self) {
        for (tuple, diff) in self.staged.drain() {
            self.pending.update(tuple, diff);
        }
    }

    /// Push a new checkpoint level.
    pub(crate) fn push_checkpoint(&mut self) {
        self.outputs_by_checkpoint.push_empty();
    }

    /// Send -1 for all outputs in the current checkpoint.
    /// Removes them from output_seen but doesn't pop the checkpoint yet.
    pub(crate) fn send_inverse(&mut self) {
        if let Some(outputs) = self.outputs_by_checkpoint.last() {
            for tuple in outputs {
                // Remove from output_seen
                self.output_seen.remove(tuple);
                // Emit -1 to staged
                self.staged.update(tuple.clone(), -1);
            }
        }
    }

    /// Update input_totals with a change (used during pop).
    pub(crate) fn update_input_total(&mut self, tuple: T, diff: Diff) {
        *self.input_totals.entry(tuple).or_insert(0) += diff;
    }

    /// Set input_total to a specific value (used by FeedbackWithIdWrapper during pop).
    pub(crate) fn set_input_total(&mut self, tuple: T, value: i64) {
        self.input_totals.insert(tuple, value);
    }

    /// Pop the checkpoint and return an iterator over its contents.
    pub(crate) fn pop_checkpoint_drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.outputs_by_checkpoint.pop().into_iter().flatten()
    }

    /// Forward a tuple that is still reachable after pop.
    /// Only forwards if input_total != 0 and not already in output_seen.
    pub(crate) fn forward_reachable(&mut self, tuple: &T) {
        let input_total = self.input_totals.get(tuple).copied().unwrap_or(0);
        if input_total != 0 && !self.output_seen.contains(tuple) {
            self.output_seen.insert(tuple.clone());
            self.staged.update(tuple.clone(), 1);
            if !self.outputs_by_checkpoint.is_empty() {
                self.outputs_by_checkpoint.push(tuple.clone());
            }
        }
    }
}

impl<T: Clone + Eq + Hash> Default for Variable<T> {
    fn default() -> Self {
        Self::new()
    }
}
