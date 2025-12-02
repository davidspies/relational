//! Differential collections - multisets that track changes.

use std::collections::HashMap;
use std::hash::Hash;

use crate::Diff;

/// A differential collection storing tuples with their multiplicities.
///
/// This is essentially a multiset that tracks how many times each tuple appears.
/// Multiplicities can be negative during intermediate computation but typically
/// should be non-negative in final results.
#[derive(Debug, Clone)]
pub struct Multiset<T> {
    /// The current state: tuple -> multiplicity
    data: HashMap<T, Diff>,
}

impl<T: Eq + Hash> Default for Multiset<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Eq + Hash> Multiset<T> {
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
        self.data.get(tuple).copied().unwrap_or(0)
    }

    /// Check if a tuple exists with positive multiplicity.
    pub fn contains(&self, tuple: &T) -> bool {
        self.get(tuple) > 0
    }

    /// Insert a tuple (increment multiplicity by 1).
    pub fn insert(&mut self, tuple: T) {
        self.update(tuple, 1);
    }

    /// Delete a tuple (decrement multiplicity by 1).
    pub fn delete(&mut self, tuple: T) {
        self.update(tuple, -1);
    }

    /// Update the multiplicity of a tuple by a diff.
    pub fn update(&mut self, tuple: T, diff: Diff) {
        if diff == 0 {
            return;
        }
        use std::collections::hash_map::Entry;
        match self.data.entry(tuple) {
            Entry::Occupied(mut e) => {
                *e.get_mut() += diff;
                if *e.get() == 0 {
                    e.remove();
                }
            }
            Entry::Vacant(e) => {
                e.insert(diff);
            }
        }
    }

    /// Iterate over tuples with positive multiplicity.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data
            .iter()
            .filter(|(_, diff)| **diff > 0)
            .map(|(tuple, _)| tuple)
    }

    /// Iterate over tuples with their multiplicities.
    pub fn iter_with_multiplicity(&self) -> impl Iterator<Item = (&T, Diff)> {
        self.data.iter().map(|(t, d)| (t, *d))
    }

    /// Clear the collection.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// Drain the collection, returning an iterator over (tuple, diff) pairs.
    /// The collection will be empty after this call.
    pub fn drain(&mut self) -> impl Iterator<Item = (T, Diff)> + '_ {
        self.data.drain()
    }
}

impl<T: Eq + Hash> IntoIterator for Multiset<T> {
    type Item = (T, Diff);
    type IntoIter = std::collections::hash_map::IntoIter<T, Diff>;

    fn into_iter(self) -> Self::IntoIter {
        self.data.into_iter()
    }
}

impl<T: Eq + Hash> FromIterator<T> for Multiset<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut coll = Multiset::new();
        for tuple in iter {
            coll.insert(tuple);
        }
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

        assert_eq!(coll.get(&1), 2);
        assert_eq!(coll.get(&2), 1);
        assert_eq!(coll.get(&3), 0);
        assert!(coll.contains(&1));
        assert!(!coll.contains(&3));
    }

    #[test]
    fn test_collection_delete() {
        let mut coll = Multiset::new();
        coll.insert(1);
        coll.insert(1);
        coll.delete(1);

        assert_eq!(coll.get(&1), 1);
        coll.delete(1);
        assert!(!coll.contains(&1));
        assert!(coll.is_empty());
    }
}
