//! Variable for tracking iterative computation state.

use std::collections::{HashMap, HashSet};

use crate::Tuple;
use crate::change::Diff;

/// A variable in an iterative computation.
///
/// Variables track a "seen set" - tuples are only emitted once when they
/// first become positive in input_totals. This ensures monotonic growth toward fixpoint.
///
/// The variable has two separate tracking mechanisms:
/// - `input_totals`: Tracks cumulative input multiplicities
/// - `output_seen`: The seen set - tuples we've emitted +1 for
///
/// During normal operation, tuples are added to output_seen when they first become
/// positive in input_totals. During pop(), we manipulate output_seen directly.
pub struct Variable<T: Tuple> {
    /// Cumulative input multiplicities.
    /// A tuple is considered "reachable" when this is positive.
    input_totals: HashMap<T, i64>,
    /// The seen set - tuples we've emitted +1 for.
    output_seen: HashSet<T>,
    /// Staged changes (not yet committed).
    staged: Vec<(T, Diff)>,
    /// Pending changes (committed, ready to be pulled).
    pending: Vec<(T, Diff)>,
    /// Stack of outputs added at each checkpoint level.
    outputs_by_checkpoint: Vec<Vec<T>>,
}

impl<T: Tuple> Variable<T> {
    /// Create a new empty variable.
    pub fn new() -> Self {
        Variable {
            input_totals: HashMap::new(),
            output_seen: HashSet::new(),
            staged: Vec::new(),
            pending: Vec::new(),
            outputs_by_checkpoint: Vec::new(),
        }
    }

    /// Add input to this variable (used during normal fixpoint).
    /// Only emits +1 if the tuple is not already in output_seen AND input_totals is positive.
    /// Once a tuple is seen, it stays in output until explicitly removed via pop().
    pub(crate) fn add_input(&mut self, tuple: T, diff: Diff) {
        // Update input_totals
        let total = self.input_totals.entry(tuple.clone()).or_insert(0);
        *total += diff.0;

        // Only add to output if:
        // 1. input_totals is now positive, AND
        // 2. not already in output_seen (seen set semantics)
        if *total > 0 && !self.output_seen.contains(&tuple) {
            self.output_seen.insert(tuple.clone());
            self.staged.push((tuple.clone(), Diff(1)));
            if let Some(level) = self.outputs_by_checkpoint.last_mut() {
                level.push(tuple);
            }
        }
        // If input_totals <= 0 or already in output_seen, do nothing
    }

    /// Backwards compatibility alias for add_input.
    pub(crate) fn add_change(&mut self, tuple: T, diff: Diff) {
        self.add_input(tuple, diff);
    }

    /// Take the pending changes (empties the buffer).
    pub(crate) fn take_changes(&mut self) -> Vec<(T, Diff)> {
        std::mem::take(&mut self.pending)
    }

    /// Commit staged changes to pending.
    pub(crate) fn commit(&mut self) {
        self.pending.append(&mut self.staged);
    }

    /// Push a new checkpoint level.
    pub(crate) fn push_checkpoint(&mut self) {
        self.outputs_by_checkpoint.push(Vec::new());
    }

    /// Send -1 for all outputs in the current checkpoint.
    /// Removes them from output_seen but doesn't pop the checkpoint yet.
    pub(crate) fn send_inverse(&mut self) {
        if let Some(outputs) = self.outputs_by_checkpoint.last() {
            for tuple in outputs {
                // Remove from output_seen
                self.output_seen.remove(tuple);
                // Emit -1 to staged
                self.staged.push((tuple.clone(), Diff(-1)));
            }
        }
    }

    /// Update input_totals with a change (used during pop).
    pub(crate) fn update_input_total(&mut self, tuple: T, diff: Diff) {
        *self.input_totals.entry(tuple).or_insert(0) += diff.0;
    }

    /// Set input_total to a specific value (used by FeedbackWithIdWrapper during pop).
    pub(crate) fn set_input_total(&mut self, tuple: T, value: i64) {
        self.input_totals.insert(tuple, value);
    }

    /// Get the last checkpoint's contents without removing it.
    pub(crate) fn get_last_checkpoint(&self) -> &[T] {
        self.outputs_by_checkpoint.last().map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Pop the checkpoint without forwarding (caller handles reachability).
    pub(crate) fn pop_checkpoint(&mut self) {
        self.outputs_by_checkpoint.pop();
    }

    /// Forward a tuple that is still reachable after pop.
    pub(crate) fn forward_reachable(&mut self, tuple: &T) {
        if !self.output_seen.contains(tuple) {
            self.output_seen.insert(tuple.clone());
            self.staged.push((tuple.clone(), Diff(1)));
            if let Some(parent) = self.outputs_by_checkpoint.last_mut() {
                parent.push(tuple.clone());
            }
        }
    }

    /// Forward +1 for a tuple if it's not in the last checkpoint and is positive in input_totals.
    pub(crate) fn forward_if_not_in_checkpoint(&mut self, tuple: &T) {
        let in_checkpoint = self
            .outputs_by_checkpoint
            .last()
            .map(|level| level.contains(tuple))
            .unwrap_or(false);

        if !in_checkpoint {
            let input_total = self.input_totals.get(tuple).copied().unwrap_or(0);
            if input_total > 0 && !self.output_seen.contains(tuple) {
                // Re-add to output_seen
                self.output_seen.insert(tuple.clone());
                self.staged.push((tuple.clone(), Diff(1)));
                // Record in parent checkpoint
                if self.outputs_by_checkpoint.len() >= 2 {
                    let parent_idx = self.outputs_by_checkpoint.len() - 2;
                    self.outputs_by_checkpoint[parent_idx].push(tuple.clone());
                }
            }
        }
    }

    /// Pop the checkpoint and forward +1 for items whose input_total is still positive.
    pub(crate) fn pop_and_forward_reachable(&mut self) {
        if let Some(outputs) = self.outputs_by_checkpoint.pop() {
            for tuple in outputs {
                let input_total = self.input_totals.get(&tuple).copied().unwrap_or(0);
                if input_total > 0 && !self.output_seen.contains(&tuple) {
                    // Still reachable - re-add to output_seen
                    self.output_seen.insert(tuple.clone());
                    self.staged.push((tuple.clone(), Diff(1)));
                    // Record in parent checkpoint
                    if let Some(parent) = self.outputs_by_checkpoint.last_mut() {
                        parent.push(tuple);
                    }
                }
            }
        }
    }
}

impl<T: Tuple> Default for Variable<T> {
    fn default() -> Self {
        Self::new()
    }
}
