//! Relational operators for differential dataflow.
//!
//! These operators define how to incrementally maintain derived relations
//! when the input relations change.

use std::collections::HashMap;

use crate::Tuple;
use crate::change::{Change, Diff};
use crate::collection::Multiset;

// ============================================================================
// Operator Definitions (non-trait based for simplicity)
// ============================================================================

/// Map operator: transforms each tuple using a function.
pub fn map<T: Tuple, U: Tuple, F: Fn(&T) -> U>(input: &Multiset<T>, f: F) -> Multiset<U> {
    let mut output = Multiset::new();
    for (tuple, diff) in input.iter_with_multiplicity() {
        output.apply_change(Change::new(f(tuple), diff));
    }
    output.compact();
    output
}

/// Incremental map: given input changes, produce output changes.
pub fn map_changes<T: Tuple, U: Tuple, F: Fn(&T) -> U>(
    changes: &[Change<T>],
    f: F,
) -> Vec<Change<U>> {
    changes
        .iter()
        .map(|c| Change::new(f(&c.tuple), c.diff))
        .collect()
}

/// Filter operator: keeps only tuples satisfying a predicate.
pub fn filter<T: Tuple, F: Fn(&T) -> bool>(input: &Multiset<T>, f: F) -> Multiset<T> {
    let mut output = Multiset::new();
    for (tuple, diff) in input.iter_with_multiplicity() {
        if f(tuple) {
            output.apply_change(Change::new(tuple.clone(), diff));
        }
    }
    output
}

/// Incremental filter: given input changes, produce output changes.
pub fn filter_changes<T: Tuple, F: Fn(&T) -> bool>(changes: &[Change<T>], f: F) -> Vec<Change<T>> {
    changes.iter().filter(|c| f(&c.tuple)).cloned().collect()
}

/// Flat map operator: transforms each tuple into zero or more tuples.
pub fn flat_map<T: Tuple, U: Tuple, I: IntoIterator<Item = U>, F: Fn(&T) -> I>(
    input: &Multiset<T>,
    f: F,
) -> Multiset<U> {
    let mut output = Multiset::new();
    for (tuple, diff) in input.iter_with_multiplicity() {
        for out in f(tuple) {
            output.apply_change(Change::new(out, diff));
        }
    }
    output.compact();
    output
}

/// Incremental flat map.
pub fn flat_map_changes<T: Tuple, U: Tuple, I: IntoIterator<Item = U>, F: Fn(&T) -> I>(
    changes: &[Change<T>],
    f: F,
) -> Vec<Change<U>> {
    let mut result = Vec::new();
    for change in changes {
        for out in f(&change.tuple) {
            result.push(Change::new(out, change.diff));
        }
    }
    result
}

/// Union operator: combines two collections.
pub fn union<T: Tuple>(a: &Multiset<T>, b: &Multiset<T>) -> Multiset<T> {
    let mut output = a.clone();
    for (tuple, diff) in b.iter_with_multiplicity() {
        output.apply_change(Change::new(tuple.clone(), diff));
    }
    output.compact();
    output
}

/// Distinct operator: ensures each tuple has multiplicity at most 1.
/// This is tricky for differential dataflow - we need to track when
/// a tuple's count crosses the 0/1 boundary.
pub fn distinct<T: Tuple>(input: &Multiset<T>) -> Multiset<T> {
    let mut output = Multiset::new();
    for (tuple, diff) in input.iter_with_multiplicity() {
        if diff.is_positive() {
            output.apply_change(Change::new(tuple.clone(), Diff::ONE));
        }
    }
    output
}

/// Incremental distinct: requires knowing the old and new multiplicities.
pub fn distinct_changes<T: Tuple>(
    old_state: &Multiset<T>,
    new_state: &Multiset<T>,
) -> Vec<Change<T>> {
    let mut changes = Vec::new();

    // Check all tuples in old state
    for (tuple, old_diff) in old_state.iter_with_multiplicity() {
        let new_diff = new_state.get(tuple);
        let was_present = old_diff.is_positive();
        let is_present = new_diff.is_positive();

        if was_present && !is_present {
            changes.push(Change::delete(tuple.clone()));
        } else if !was_present && is_present {
            changes.push(Change::insert(tuple.clone()));
        }
    }

    // Check tuples only in new state
    for (tuple, new_diff) in new_state.iter_with_multiplicity() {
        if !old_state.data().contains_key(tuple) && new_diff.is_positive() {
            changes.push(Change::insert(tuple.clone()));
        }
    }

    changes
}

/// Negate operator: flips all multiplicities.
pub fn negate<T: Tuple>(input: &Multiset<T>) -> Multiset<T> {
    let mut output = Multiset::new();
    for (tuple, diff) in input.iter_with_multiplicity() {
        output.apply_change(Change::new(tuple.clone(), -diff));
    }
    output
}

/// Incremental negate.
pub fn negate_changes<T: Tuple>(changes: &[Change<T>]) -> Vec<Change<T>> {
    changes.iter().map(|c| c.negate()).collect()
}

/// Join operator for two collections with key extraction.
/// join(A, B, key_a, key_b) produces (a, b) for all (a in A, b in B) where key_a(a) == key_b(b)
pub fn join<A: Tuple, B: Tuple, K: Tuple, FA: Fn(&A) -> K, FB: Fn(&B) -> K>(
    left: &Multiset<A>,
    right: &Multiset<B>,
    key_left: FA,
    key_right: FB,
) -> Multiset<(A, B)> {
    // Build index on right side
    let mut right_index: HashMap<K, Vec<(B, Diff)>> = HashMap::new();
    for (tuple, diff) in right.iter_with_multiplicity() {
        let key = key_right(tuple);
        right_index
            .entry(key)
            .or_default()
            .push((tuple.clone(), diff));
    }

    // Probe with left side
    let mut output = Multiset::new();
    for (left_tuple, left_diff) in left.iter_with_multiplicity() {
        let key = key_left(left_tuple);
        if let Some(matches) = right_index.get(&key) {
            for (right_tuple, right_diff) in matches {
                let out_diff = Diff(left_diff.0 * right_diff.0);
                output.apply_change(Change::new(
                    (left_tuple.clone(), right_tuple.clone()),
                    out_diff,
                ));
            }
        }
    }
    output.compact();
    output
}

/// Incremental join: process changes to left or right side.
/// Requires maintaining the current state of both sides.
pub fn join_changes_left<A: Tuple, B: Tuple, K: Tuple, FA: Fn(&A) -> K, FB: Fn(&B) -> K>(
    left_changes: &[Change<A>],
    right_state: &Multiset<B>,
    key_left: FA,
    key_right: FB,
) -> Vec<Change<(A, B)>> {
    // Build index on right side
    let mut right_index: HashMap<K, Vec<(B, Diff)>> = HashMap::new();
    for (tuple, diff) in right_state.iter_with_multiplicity() {
        let key = key_right(tuple);
        right_index
            .entry(key)
            .or_default()
            .push((tuple.clone(), diff));
    }

    // Process left changes
    let mut output = Vec::new();
    for change in left_changes {
        let key = key_left(&change.tuple);
        if let Some(matches) = right_index.get(&key) {
            for (right_tuple, right_diff) in matches {
                let out_diff = Diff(change.diff.0 * right_diff.0);
                output.push(Change::new(
                    (change.tuple.clone(), right_tuple.clone()),
                    out_diff,
                ));
            }
        }
    }
    output
}

pub fn join_changes_right<A: Tuple, B: Tuple, K: Tuple, FA: Fn(&A) -> K, FB: Fn(&B) -> K>(
    left_state: &Multiset<A>,
    right_changes: &[Change<B>],
    key_left: FA,
    key_right: FB,
) -> Vec<Change<(A, B)>> {
    // Build index on left side
    let mut left_index: HashMap<K, Vec<(A, Diff)>> = HashMap::new();
    for (tuple, diff) in left_state.iter_with_multiplicity() {
        let key = key_left(tuple);
        left_index
            .entry(key)
            .or_default()
            .push((tuple.clone(), diff));
    }

    // Process right changes
    let mut output = Vec::new();
    for change in right_changes {
        let key = key_right(&change.tuple);
        if let Some(matches) = left_index.get(&key) {
            for (left_tuple, left_diff) in matches {
                let out_diff = Diff(change.diff.0 * left_diff.0);
                output.push(Change::new(
                    (left_tuple.clone(), change.tuple.clone()),
                    out_diff,
                ));
            }
        }
    }
    output
}

/// Semijoin: keeps tuples from left that have a match in right.
pub fn semijoin<A: Tuple, B: Tuple, K: Tuple, FA: Fn(&A) -> K, FB: Fn(&B) -> K>(
    left: &Multiset<A>,
    right: &Multiset<B>,
    key_left: FA,
    key_right: FB,
) -> Multiset<A> {
    // Get keys present in right
    let mut right_keys: HashMap<K, bool> = HashMap::new();
    for (tuple, diff) in right.iter_with_multiplicity() {
        if diff.is_positive() {
            right_keys.insert(key_right(tuple), true);
        }
    }

    // Filter left
    let mut output = Multiset::new();
    for (tuple, diff) in left.iter_with_multiplicity() {
        if right_keys.contains_key(&key_left(tuple)) {
            output.apply_change(Change::new(tuple.clone(), diff));
        }
    }
    output
}

/// Antijoin: keeps tuples from left that have NO match in right.
pub fn antijoin<A: Tuple, B: Tuple, K: Tuple, FA: Fn(&A) -> K, FB: Fn(&B) -> K>(
    left: &Multiset<A>,
    right: &Multiset<B>,
    key_left: FA,
    key_right: FB,
) -> Multiset<A> {
    // Get keys present in right
    let mut right_keys: HashMap<K, bool> = HashMap::new();
    for (tuple, diff) in right.iter_with_multiplicity() {
        if diff.is_positive() {
            right_keys.insert(key_right(tuple), true);
        }
    }

    // Filter left (keep those NOT in right)
    let mut output = Multiset::new();
    for (tuple, diff) in left.iter_with_multiplicity() {
        if !right_keys.contains_key(&key_left(tuple)) {
            output.apply_change(Change::new(tuple.clone(), diff));
        }
    }
    output
}

/// Group and aggregate operator.
/// Groups tuples by key and applies an aggregation function.
pub fn aggregate<
    T: Tuple,
    K: Tuple,
    V: Tuple,
    A: Tuple,
    FK: Fn(&T) -> K,
    FV: Fn(&T) -> V,
    FA: Fn(K, &[(V, Diff)]) -> A,
>(
    input: &Multiset<T>,
    key_fn: FK,
    value_fn: FV,
    agg_fn: FA,
) -> Multiset<A> {
    // Group by key
    let mut groups: HashMap<K, Vec<(V, Diff)>> = HashMap::new();
    for (tuple, diff) in input.iter_with_multiplicity() {
        let key = key_fn(tuple);
        let value = value_fn(tuple);
        groups.entry(key).or_default().push((value, diff));
    }

    // Apply aggregation
    let mut output = Multiset::new();
    for (key, values) in groups {
        let result = agg_fn(key, &values);
        output.insert(result);
    }
    output
}

/// Count aggregation helper.
pub fn count<K: Tuple>(key: K, values: &[((), Diff)]) -> (K, i64) {
    let total: i64 = values.iter().map(|(_, d)| d.0).sum();
    (key, total)
}

/// Sum aggregation helper.
pub fn sum<K: Tuple>(key: K, values: &[(i64, Diff)]) -> (K, i64) {
    let total: i64 = values.iter().map(|(v, d)| v * d.0).sum();
    (key, total)
}

/// Min aggregation helper (returns None if empty).
pub fn min<K: Tuple, V: Tuple + Ord>(key: K, values: &[(V, Diff)]) -> Option<(K, V)> {
    values
        .iter()
        .filter(|(_, d)| d.is_positive())
        .map(|(v, _)| v)
        .min()
        .cloned()
        .map(|v| (key, v))
}

/// Max aggregation helper (returns None if empty).
pub fn max<K: Tuple, V: Tuple + Ord>(key: K, values: &[(V, Diff)]) -> Option<(K, V)> {
    values
        .iter()
        .filter(|(_, d)| d.is_positive())
        .map(|(v, _)| v)
        .max()
        .cloned()
        .map(|v| (key, v))
}

// ============================================================================
// BTreeMap-based Group Aggregations
// ============================================================================

use std::collections::BTreeMap;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map() {
        let input: Multiset<i32> = [1, 2, 3].into_iter().collect();
        let output = map(&input, |x| x * 2);

        assert!(output.contains(&2));
        assert!(output.contains(&4));
        assert!(output.contains(&6));
    }

    #[test]
    fn test_filter() {
        let input: Multiset<i32> = [1, 2, 3, 4, 5].into_iter().collect();
        let output = filter(&input, |x| x % 2 == 0);

        assert!(!output.contains(&1));
        assert!(output.contains(&2));
        assert!(!output.contains(&3));
        assert!(output.contains(&4));
    }

    #[test]
    fn test_join() {
        let left: Multiset<(i32, &str)> = [(1, "a"), (2, "b"), (3, "c")].into_iter().collect();
        let right: Multiset<(i32, i32)> = [(1, 10), (2, 20), (4, 40)].into_iter().collect();

        let joined = join(&left, &right, |(k, _)| *k, |(k, _)| *k);

        assert!(joined.contains(&((1, "a"), (1, 10))));
        assert!(joined.contains(&((2, "b"), (2, 20))));
        assert!(!joined.contains(&((3, "c"), (3, 30))));
    }

    #[test]
    fn test_distinct() {
        let mut input = Multiset::new();
        input.insert(1);
        input.insert(1);
        input.insert(2);

        let output = distinct(&input);

        assert_eq!(output.get(&1), Diff::ONE);
        assert_eq!(output.get(&2), Diff::ONE);
    }

    #[test]
    fn test_union() {
        let a: Multiset<i32> = [1, 2].into_iter().collect();
        let b: Multiset<i32> = [2, 3].into_iter().collect();

        let result = union(&a, &b);

        assert_eq!(result.get(&1), Diff::ONE);
        assert_eq!(result.get(&2), Diff(2)); // 2 appears in both
        assert_eq!(result.get(&3), Diff::ONE);
    }
}
