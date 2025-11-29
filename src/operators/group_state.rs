//! BTreeMap-based group aggregation state for incremental min/max.

use std::collections::{BTreeMap, HashMap};

use crate::change::{Change, Diff};
use crate::collection::Multiset;
use crate::Tuple;

/// State for tracking group_max incrementally.
/// For each key, maintains a BTreeMap of values to their multiplicities.
#[derive(Clone, Debug)]
pub struct GroupMaxState<K: Tuple, V: Tuple + Ord> {
    /// For each key, a BTreeMap of value -> multiplicity
    groups: HashMap<K, BTreeMap<V, Diff>>,
}

impl<K: Tuple, V: Tuple + Ord> Default for GroupMaxState<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Tuple, V: Tuple + Ord> GroupMaxState<K, V> {
    pub fn new() -> Self {
        GroupMaxState {
            groups: HashMap::new(),
        }
    }

    /// Get the current max for a key (if any).
    fn get_max(&self, key: &K) -> Option<V> {
        self.groups.get(key).and_then(|btree| {
            // Find the largest key with positive multiplicity
            btree
                .iter()
                .rev()
                .find(|(_, diff)| diff.is_positive())
                .map(|(v, _)| v.clone())
        })
    }

    /// Apply a change and return any output changes.
    /// Returns (old_max, new_max) if the max changed.
    pub fn apply_change(&mut self, key: K, value: V, diff: Diff) -> Option<(Option<V>, Option<V>)> {
        let old_max = self.get_max(&key);

        let btree = self.groups.entry(key.clone()).or_default();
        let entry = btree.entry(value).or_insert(Diff::ZERO);
        *entry += diff;

        // Clean up zero entries
        btree.retain(|_, d| !d.is_zero());
        if btree.is_empty() {
            self.groups.remove(&key);
        }

        let new_max = self.get_max(&key);

        if old_max != new_max {
            Some((old_max, new_max))
        } else {
            None
        }
    }

    /// Compute the full output as a Multiset.
    pub fn to_multiset(&self) -> Multiset<(K, V)> {
        let mut result = Multiset::new();
        for (key, btree) in &self.groups {
            if let Some((max_val, _)) = btree.iter().rev().find(|(_, d)| d.is_positive()) {
                result.insert((key.clone(), max_val.clone()));
            }
        }
        result
    }
}

/// State for tracking group_min incrementally.
#[derive(Clone, Debug)]
pub struct GroupMinState<K: Tuple, V: Tuple + Ord> {
    groups: HashMap<K, BTreeMap<V, Diff>>,
}

impl<K: Tuple, V: Tuple + Ord> Default for GroupMinState<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Tuple, V: Tuple + Ord> GroupMinState<K, V> {
    pub fn new() -> Self {
        GroupMinState {
            groups: HashMap::new(),
        }
    }

    fn get_min(&self, key: &K) -> Option<V> {
        self.groups.get(key).and_then(|btree| {
            btree
                .iter()
                .find(|(_, diff)| diff.is_positive())
                .map(|(v, _)| v.clone())
        })
    }

    pub fn apply_change(&mut self, key: K, value: V, diff: Diff) -> Option<(Option<V>, Option<V>)> {
        let old_min = self.get_min(&key);

        let btree = self.groups.entry(key.clone()).or_default();
        let entry = btree.entry(value).or_insert(Diff::ZERO);
        *entry += diff;

        btree.retain(|_, d| !d.is_zero());
        if btree.is_empty() {
            self.groups.remove(&key);
        }

        let new_min = self.get_min(&key);

        if old_min != new_min {
            Some((old_min, new_min))
        } else {
            None
        }
    }

    pub fn to_multiset(&self) -> Multiset<(K, V)> {
        let mut result = Multiset::new();
        for (key, btree) in &self.groups {
            if let Some((min_val, _)) = btree.iter().find(|(_, d)| d.is_positive()) {
                result.insert((key.clone(), min_val.clone()));
            }
        }
        result
    }
}

/// Build initial GroupMaxState from a Multiset.
pub fn group_max_init<T: Tuple, K: Tuple, V: Tuple + Ord>(
    input: &Multiset<T>,
    key_fn: impl Fn(&T) -> K,
    value_fn: impl Fn(&T) -> V,
) -> (GroupMaxState<K, V>, Multiset<(K, V)>) {
    let mut state = GroupMaxState::new();
    for (tuple, diff) in input.iter_with_multiplicity() {
        let key = key_fn(tuple);
        let value = value_fn(tuple);
        state.apply_change(key, value, diff);
    }
    let output = state.to_multiset();
    (state, output)
}

/// Build initial GroupMinState from a Multiset.
pub fn group_min_init<T: Tuple, K: Tuple, V: Tuple + Ord>(
    input: &Multiset<T>,
    key_fn: impl Fn(&T) -> K,
    value_fn: impl Fn(&T) -> V,
) -> (GroupMinState<K, V>, Multiset<(K, V)>) {
    let mut state = GroupMinState::new();
    for (tuple, diff) in input.iter_with_multiplicity() {
        let key = key_fn(tuple);
        let value = value_fn(tuple);
        state.apply_change(key, value, diff);
    }
    let output = state.to_multiset();
    (state, output)
}

/// Incrementally update GroupMaxState and produce output changes.
pub fn group_max_changes<T: Tuple, K: Tuple, V: Tuple + Ord>(
    state: &mut GroupMaxState<K, V>,
    changes: &[Change<T>],
    key_fn: impl Fn(&T) -> K,
    value_fn: impl Fn(&T) -> V,
) -> Vec<Change<(K, V)>> {
    let mut output = Vec::new();

    for change in changes {
        let key = key_fn(&change.tuple);
        let value = value_fn(&change.tuple);

        if let Some((old_max, new_max)) = state.apply_change(key.clone(), value, change.diff) {
            // Emit changes
            if let Some(old) = old_max {
                output.push(Change::delete((key.clone(), old)));
            }
            if let Some(new) = new_max {
                output.push(Change::insert((key, new)));
            }
        }
    }

    output
}

/// Incrementally update GroupMinState and produce output changes.
pub fn group_min_changes<T: Tuple, K: Tuple, V: Tuple + Ord>(
    state: &mut GroupMinState<K, V>,
    changes: &[Change<T>],
    key_fn: impl Fn(&T) -> K,
    value_fn: impl Fn(&T) -> V,
) -> Vec<Change<(K, V)>> {
    let mut output = Vec::new();

    for change in changes {
        let key = key_fn(&change.tuple);
        let value = value_fn(&change.tuple);

        if let Some((old_min, new_min)) = state.apply_change(key.clone(), value, change.diff) {
            if let Some(old) = old_min {
                output.push(Change::delete((key.clone(), old)));
            }
            if let Some(new) = new_min {
                output.push(Change::insert((key, new)));
            }
        }
    }

    output
}
