//! Differential collections - multisets that track changes.

use std::collections::HashMap;

use crate::change::{Change, Diff};
use crate::Tuple;

/// A differential collection storing tuples with their multiplicities.
///
/// This is essentially a multiset that tracks how many times each tuple appears.
/// Multiplicities can be negative during intermediate computation but typically
/// should be non-negative in final results.
#[derive(Debug, Clone)]
pub struct Multiset<T: Tuple> {
    /// The current state: tuple -> multiplicity
    data: HashMap<T, Diff>,
}

impl<T: Tuple> Default for Multiset<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Tuple> Multiset<T> {
    /// Create an empty collection.
    pub fn new() -> Self {
        Multiset {
            data: HashMap::new(),
        }
    }


    /// Check if the collection is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get the number of distinct tuples (ignoring multiplicity).
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Get the multiplicity of a tuple.
    pub fn get(&self, tuple: &T) -> Diff {
        self.data.get(tuple).copied().unwrap_or(Diff::ZERO)
    }

    /// Check if a tuple exists with positive multiplicity.
    pub fn contains(&self, tuple: &T) -> bool {
        self.get(tuple).is_positive()
    }

    /// Insert a tuple (increment multiplicity by 1).
    pub fn insert(&mut self, tuple: T) {
        self.apply_change(Change::insert(tuple));
    }

    /// Delete a tuple (decrement multiplicity by 1).
    pub fn delete(&mut self, tuple: T) {
        self.apply_change(Change::delete(tuple));
    }

    /// Apply a single change to the collection.
    pub fn apply_change(&mut self, change: Change<T>) {
        if change.diff.is_zero() {
            return;
        }
        let entry = self.data.entry(change.tuple).or_insert(Diff::ZERO);
        *entry += change.diff;
        // Clean up zero entries
        if entry.is_zero() {
            // We need to re-lookup since we can't remove while holding the mutable ref
            // This is a bit awkward, but necessary
        }
    }

    /// Apply a batch of changes to the collection.
    pub fn apply_changes<I: IntoIterator<Item = Change<T>>>(&mut self, changes: I) {
        for change in changes {
            self.apply_change(change);
        }
        self.compact();
    }

    /// Remove all zero-multiplicity entries.
    pub fn compact(&mut self) {
        self.data.retain(|_, diff| !diff.is_zero());
    }

    /// Iterate over tuples with positive multiplicity.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data
            .iter()
            .filter(|(_, diff)| diff.is_positive())
            .map(|(tuple, _)| tuple)
    }

    /// Iterate over tuples with their multiplicities.
    pub fn iter_with_multiplicity(&self) -> impl Iterator<Item = (&T, Diff)> {
        self.data.iter().map(|(t, d)| (t, *d))
    }

    /// Iterate over tuples, repeating each one by its positive multiplicity.
    pub fn iter_flat(&self) -> impl Iterator<Item = &T> {
        self.data.iter().flat_map(|(tuple, diff)| {
            let count = if diff.is_positive() { diff.0 as usize } else { 0 };
            std::iter::repeat_n(tuple, count)
        })
    }

    /// Convert the collection to a Vec of its tuples (with positive multiplicity).
    pub fn to_vec(&self) -> Vec<T> {
        self.iter().cloned().collect()
    }

    /// Get the internal data map.
    pub fn data(&self) -> &HashMap<T, Diff> {
        &self.data
    }

    /// Clear the collection.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// Compute the changes needed to transform from this collection to another.
    pub fn diff(&self, other: &Multiset<T>) -> Vec<Change<T>> {
        let mut changes = Vec::new();

        // For each tuple in self, compute the difference
        for (tuple, &self_diff) in &self.data {
            let other_diff = other.get(tuple);
            let delta = other_diff - self_diff;
            if !delta.is_zero() {
                changes.push(Change::new(tuple.clone(), delta));
            }
        }

        // For tuples only in other
        for (tuple, &other_diff) in &other.data {
            if !self.data.contains_key(tuple) {
                changes.push(Change::new(tuple.clone(), other_diff));
            }
        }

        changes
    }

    /// Create a snapshot of the current state.
    pub fn snapshot(&self) -> Multiset<T> {
        self.clone()
    }
}

impl<T: Tuple> FromIterator<T> for Multiset<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut coll = Multiset::new();
        for tuple in iter {
            coll.insert(tuple);
        }
        coll
    }
}

impl<T: Tuple> FromIterator<Change<T>> for Multiset<T> {
    fn from_iter<I: IntoIterator<Item = Change<T>>>(iter: I) -> Self {
        let mut coll = Multiset::new();
        coll.apply_changes(iter);
        coll
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collection_basic() {
        let mut coll = Multiset::new();
        coll.insert(1);
        coll.insert(2);
        coll.insert(1);

        assert_eq!(coll.get(&1), Diff(2));
        assert_eq!(coll.get(&2), Diff(1));
        assert_eq!(coll.get(&3), Diff(0));
        assert!(coll.contains(&1));
        assert!(!coll.contains(&3));
    }

    #[test]
    fn test_collection_delete() {
        let mut coll = Multiset::new();
        coll.insert(1);
        coll.insert(1);
        coll.delete(1);

        assert_eq!(coll.get(&1), Diff(1));
        coll.delete(1);
        coll.compact();
        assert!(!coll.contains(&1));
    }

    #[test]
    fn test_collection_diff() {
        let coll1: Multiset<i32> = [1, 2, 3].into_iter().collect();
        let coll2: Multiset<i32> = [2, 3, 4].into_iter().collect();

        let changes = coll1.diff(&coll2);
        let mut result = coll1.clone();
        result.apply_changes(changes);

        assert_eq!(result.to_vec().len(), coll2.to_vec().len());
    }
}
