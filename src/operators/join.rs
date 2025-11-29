//! Join operators: join, semijoin, antijoin.

use std::collections::HashMap;

use crate::change::{Change, Diff};
use crate::collection::Multiset;
use crate::Tuple;

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
