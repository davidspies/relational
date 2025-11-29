//! Basic operators: map, filter, flat_map, union, distinct, negate.

use crate::change::{Change, Diff};
use crate::collection::Multiset;
use crate::Tuple;

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
