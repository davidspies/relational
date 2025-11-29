//! Variable for tracking iterative computation state.

use std::collections::HashMap;

use crate::change::Diff;
use crate::Tuple;

/// A variable in an iterative computation.
///
/// Variables track a "seen set" - tuples are only emitted once when they
/// first become positive. This ensures monotonic growth toward fixpoint.
pub struct Variable<T: Tuple> {
    /// All tuples with their total multiplicities.
    pub(super) totals: HashMap<T, i64>,
    /// Changes from the current iteration (to be delivered to readers).
    current_changes: Vec<(T, Diff)>,
    /// Accumulated input multiplicities (from upstream computation).
    /// This tracks what would be the "ground truth" if we re-ran from scratch.
    input_counts: HashMap<T, i64>,
    /// Stack of outputs produced at each checkpoint level.
    /// On pop, we revert these and re-check reachability via input_counts.
    outputs_by_checkpoint: Vec<Vec<T>>,
}

impl<T: Tuple> Variable<T> {
    /// Create a new empty variable.
    pub fn new() -> Self {
        Variable {
            totals: HashMap::new(),
            current_changes: Vec::new(),
            input_counts: HashMap::new(),
            outputs_by_checkpoint: Vec::new(),
        }
    }

    /// Insert an initial value.
    pub fn insert(&mut self, tuple: T) {
        self.add_change(tuple, Diff(1));
    }

    /// Add a change to this variable.
    /// Only emits if the tuple transitions to/from positive.
    pub fn add_change(&mut self, tuple: T, diff: Diff) {
        // Track input counts for checkpoint reachability
        *self.input_counts.entry(tuple.clone()).or_insert(0) += diff.0;

        let total = self.totals.entry(tuple.clone()).or_insert(0);
        let was_positive = *total > 0;
        *total += diff.0;
        let is_positive = *total > 0;

        if !was_positive && is_positive {
            // Track this output for the current checkpoint level
            if let Some(level) = self.outputs_by_checkpoint.last_mut() {
                level.push(tuple.clone());
            }
            self.current_changes.push((tuple, Diff(1)));
        } else if was_positive && !is_positive {
            self.current_changes.push((tuple, Diff(-1)));
        }
    }

    /// Take the current changes (empties the buffer).
    pub fn take_changes(&mut self) -> Vec<(T, Diff)> {
        std::mem::take(&mut self.current_changes)
    }

    /// Check if there are pending changes.
    pub fn has_changes(&self) -> bool {
        !self.current_changes.is_empty()
    }

    /// Get all positive tuples.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.totals
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

    /// Pop a checkpoint level.
    ///
    /// This reverts outputs from this checkpoint and re-checks reachability.
    /// Returns tuples that should be re-added (still reachable via input_counts).
    pub fn pop_checkpoint(&mut self) -> Vec<T> {
        let outputs = self.outputs_by_checkpoint.pop().unwrap_or_default();
        let mut to_readd = Vec::new();

        for tuple in outputs {
            // Revert this output
            let total = self.totals.get_mut(&tuple).unwrap();
            let was_positive = *total > 0;
            *total -= 1; // Undo the +1 emission

            if was_positive && *total <= 0 {
                // Emit deletion
                self.current_changes.push((tuple.clone(), Diff(-1)));
            }

            // Check if still reachable via input_counts
            let input_count = self.input_counts.get(&tuple).copied().unwrap_or(0);
            if input_count > 0 {
                to_readd.push(tuple);
            }
        }

        to_readd
    }

    /// Re-add tuples that are still reachable after a pop.
    pub fn readd(&mut self, tuples: Vec<T>) {
        for tuple in tuples {
            let total = self.totals.entry(tuple.clone()).or_insert(0);
            let was_positive = *total > 0;
            *total += 1;

            if !was_positive && *total > 0 {
                // Track in current checkpoint level
                if let Some(level) = self.outputs_by_checkpoint.last_mut() {
                    level.push(tuple.clone());
                }
                self.current_changes.push((tuple, Diff(1)));
            }
        }
    }

    /// Update input_count for a single tuple.
    pub fn update_input_count(&mut self, tuple: T, diff: Diff) {
        let count = self.input_counts.entry(tuple).or_insert(0);
        *count += diff.0;
    }

    /// Re-add all tuples that have positive input_counts but aren't in totals.
    /// Called after pull_and_readd updates input_counts.
    pub fn readd_reachable(&mut self) {
        // Find tuples that are reachable (input_counts > 0) but not in totals
        let to_readd: Vec<T> = self
            .input_counts
            .iter()
            .filter(|(tuple, count)| {
                **count > 0 && self.totals.get(*tuple).copied().unwrap_or(0) <= 0
            })
            .map(|(tuple, _)| tuple.clone())
            .collect();

        self.readd(to_readd);
    }
}

impl<T: Tuple> Default for Variable<T> {
    fn default() -> Self {
        Self::new()
    }
}
