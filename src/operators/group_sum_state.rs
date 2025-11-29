//! HashMap-based group aggregation state for incremental sum.

use std::collections::HashMap;

use crate::change::{Change, Diff};
use crate::collection::Multiset;
use crate::Tuple;

/// State for tracking group_sum incrementally.
/// For each key, maintains the running sum of (value * multiplicity).
#[derive(Clone, Debug)]
pub struct GroupSumState<K: Tuple> {
    /// For each key, the running sum
    sums: HashMap<K, i64>,
}

impl<K: Tuple> Default for GroupSumState<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Tuple> GroupSumState<K> {
    pub fn new() -> Self {
        GroupSumState {
            sums: HashMap::new(),
        }
    }

    /// Apply a change and return (old_sum, new_sum) if the output changed.
    pub fn apply_change(&mut self, key: K, value: i64, diff: Diff) -> Option<(i64, i64)> {
        let old_sum = *self.sums.get(&key).unwrap_or(&0);
        let delta = value * diff.0;
        let new_sum = old_sum + delta;

        if new_sum == 0 {
            self.sums.remove(&key);
        } else {
            self.sums.insert(key, new_sum);
        }

        if old_sum != new_sum {
            Some((old_sum, new_sum))
        } else {
            None
        }
    }

    /// Compute the full output as a Multiset.
    pub fn to_multiset(&self) -> Multiset<(K, i64)> {
        let mut result = Multiset::new();
        for (key, &sum) in &self.sums {
            if sum != 0 {
                result.insert((key.clone(), sum));
            }
        }
        result
    }
}

/// Build initial GroupSumState from a Multiset.
pub fn group_sum_init<T: Tuple, K: Tuple>(
    input: &Multiset<T>,
    key_fn: impl Fn(&T) -> K,
    value_fn: impl Fn(&T) -> i64,
) -> (GroupSumState<K>, Multiset<(K, i64)>) {
    let mut state = GroupSumState::new();
    for (tuple, diff) in input.iter_with_multiplicity() {
        let key = key_fn(tuple);
        let value = value_fn(tuple);
        state.apply_change(key, value, diff);
    }
    let output = state.to_multiset();
    (state, output)
}

/// Incrementally update GroupSumState and produce output changes.
pub fn group_sum_changes<T: Tuple, K: Tuple>(
    state: &mut GroupSumState<K>,
    changes: &[Change<T>],
    key_fn: impl Fn(&T) -> K,
    value_fn: impl Fn(&T) -> i64,
) -> Vec<Change<(K, i64)>> {
    let mut output = Vec::new();

    for change in changes {
        let key = key_fn(&change.tuple);
        let value = value_fn(&change.tuple);

        if let Some((old_sum, new_sum)) = state.apply_change(key.clone(), value, change.diff) {
            // Emit changes: remove old, add new (if non-zero)
            if old_sum != 0 {
                output.push(Change::delete((key.clone(), old_sum)));
            }
            if new_sum != 0 {
                output.push(Change::insert((key, new_sum)));
            }
        }
    }

    output
}
