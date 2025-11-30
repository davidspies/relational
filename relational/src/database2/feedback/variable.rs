//! Variable for tracking iterative computation state.

use std::collections::HashMap;

use crate::Tuple;
use crate::change::Diff;

/// A variable in an iterative computation.
///
/// Variables track a "seen set" - tuples are only emitted once when they
/// first become positive in input_totals. This ensures monotonic growth toward fixpoint.
///
/// The variable has two separate tracking mechanisms:
/// - `input_totals`: Tracks cumulative input multiplicities (the "seen set")
/// - `output_state`: The actual output state (what's visible to downstream)
///
/// During normal operation, tuples are added to output when they first become
/// positive in input_totals. During pop(), we manipulate output_state directly.
pub struct Variable<T: Tuple> {
    /// Cumulative input multiplicities - the "seen set".
    /// A tuple is considered "seen" when this is positive.
    input_totals: HashMap<T, i64>,
    /// The actual output state - what downstream operators see.
    output_state: HashMap<T, i64>,
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
            output_state: HashMap::new(),
            staged: Vec::new(),
            pending: Vec::new(),
            outputs_by_checkpoint: Vec::new(),
        }
    }

    /// Insert an initial value.
    pub fn insert(&mut self, tuple: T) {
        self.add_input(tuple, Diff(1));
    }

    /// Add input to this variable (used during normal fixpoint).
    /// Only emits +1 if the tuple is not already in output_state AND input_totals is positive.
    /// Once a tuple is seen, it stays in output until explicitly removed via pop().
    pub fn add_input(&mut self, tuple: T, diff: Diff) {
        // Update input_totals
        let total = self.input_totals.entry(tuple.clone()).or_insert(0);
        *total += diff.0;

        // Only add to output if:
        // 1. input_totals is now positive, AND
        // 2. not already in output (seen set semantics)
        if *total > 0 {
            let output = self.output_state.entry(tuple.clone()).or_insert(0);
            if *output <= 0 {
                *output = 1;
                self.staged.push((tuple.clone(), Diff(1)));
                if let Some(level) = self.outputs_by_checkpoint.last_mut() {
                    level.push(tuple);
                }
            }
        }
        // If input_totals <= 0 or already in output, do nothing
    }

    /// Backwards compatibility alias for add_input.
    pub fn add_change(&mut self, tuple: T, diff: Diff) {
        self.add_input(tuple, diff);
    }

    /// Take the pending changes (empties the buffer).
    pub fn take_changes(&mut self) -> Vec<(T, Diff)> {
        std::mem::take(&mut self.pending)
    }

    /// Commit staged changes to pending.
    pub fn commit(&mut self) {
        self.pending.append(&mut self.staged);
    }

    /// Check if there are pending changes.
    pub fn has_changes(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Check if there are staged changes.
    pub fn has_staged(&self) -> bool {
        !self.staged.is_empty()
    }

    /// Get all positive tuples in output_state.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.output_state
            .iter()
            .filter(|&(_, &count)| count > 0)
            .map(|(t, _)| t)
    }

    /// Collect all positive tuples into a Vec.
    pub fn collect(&self) -> Vec<T> {
        self.iter().cloned().collect()
    }

    /// Push a new checkpoint level.
    pub fn push_checkpoint(&mut self) {
        self.outputs_by_checkpoint.push(Vec::new());
    }

    /// Send -1 for all outputs in the current checkpoint.
    /// Removes them from output_state but doesn't pop the checkpoint yet.
    pub fn send_inverse(&mut self) {
        if let Some(outputs) = self.outputs_by_checkpoint.last() {
            for tuple in outputs {
                // Remove from output_state
                if let Some(count) = self.output_state.get_mut(tuple) {
                    *count -= 1;
                }
                // Emit -1 to staged
                self.staged.push((tuple.clone(), Diff(-1)));
            }
        }
    }

    /// Update input_totals with a change (used during pop).
    pub fn update_input_total(&mut self, tuple: T, diff: Diff) {
        *self.input_totals.entry(tuple).or_insert(0) += diff.0;
    }

    /// Forward +1 for a tuple if it's not in the last checkpoint and is positive in input_totals.
    pub fn forward_if_not_in_checkpoint(&mut self, tuple: &T) {
        let in_checkpoint = self
            .outputs_by_checkpoint
            .last()
            .map(|level| level.contains(tuple))
            .unwrap_or(false);

        if !in_checkpoint {
            let input_total = self.input_totals.get(tuple).copied().unwrap_or(0);
            if input_total > 0 {
                // Re-add to output state if not already positive
                let output = self.output_state.entry(tuple.clone()).or_insert(0);
                if *output <= 0 {
                    *output += 1;
                    self.staged.push((tuple.clone(), Diff(1)));
                    // Record in parent checkpoint
                    if self.outputs_by_checkpoint.len() >= 2 {
                        let parent_idx = self.outputs_by_checkpoint.len() - 2;
                        self.outputs_by_checkpoint[parent_idx].push(tuple.clone());
                    }
                }
            }
        }
    }

    /// Pop the checkpoint and forward +1 for items whose input_total is still positive.
    pub fn pop_and_forward_reachable(&mut self) {
        if let Some(outputs) = self.outputs_by_checkpoint.pop() {
            for tuple in outputs {
                let input_total = self.input_totals.get(&tuple).copied().unwrap_or(0);
                if input_total > 0 {
                    // Still reachable - re-add to output state
                    let output = self.output_state.entry(tuple.clone()).or_insert(0);
                    if *output <= 0 {
                        *output += 1;
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

    /// Debug: get input_total for a tuple.
    pub fn debug_input_total(&self, tuple: &T) -> i64 {
        self.input_totals.get(tuple).copied().unwrap_or(0)
    }

    /// Debug: get output_state for a tuple.
    pub fn debug_output(&self, tuple: &T) -> i64 {
        self.output_state.get(tuple).copied().unwrap_or(0)
    }

    /// Debug: get checkpoint depth.
    pub fn debug_checkpoint_depth(&self) -> usize {
        self.outputs_by_checkpoint.len()
    }
}

impl<T: Tuple> Default for Variable<T> {
    fn default() -> Self {
        Self::new()
    }
}
