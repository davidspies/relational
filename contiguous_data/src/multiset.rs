//! Differential collections - multisets that track changes.

use std::hash::Hash;

use ahash::AHashMap;
use derive_where::derive_where;

use crate::Diff;

/// A differential collection storing tuples with their multiplicities.
///
/// This is essentially a multiset that tracks how many times each tuple appears.
/// Multiplicities can be negative during intermediate computation but typically
/// should be non-negative in final results.
#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct Multiset<T> {
    /// The current state: tuple -> multiplicity
    data: AHashMap<T, Diff>,
}

impl<T: Eq + Hash> Multiset<T> {
    /// Create an empty collection.
    pub fn new() -> Self {
        Self::default()
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

    /// Check if a tuple exists with non-zero multiplicity.
    pub fn contains(&self, tuple: &T) -> bool {
        self.get(tuple) != 0
    }

    pub fn remove(&mut self, tuple: &T) -> Diff {
        self.data.remove(tuple).unwrap_or(0)
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

    /// Iterate over tuples with non-zero multiplicity.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.keys()
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

impl<T: Eq + Hash> FromIterator<(T, Diff)> for Multiset<T> {
    fn from_iter<I: IntoIterator<Item = (T, Diff)>>(iter: I) -> Self {
        let mut coll = Multiset::new();
        for (tuple, diff) in iter {
            coll.update(tuple, diff);
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
        coll.update(1, 1);
        coll.update(2, 1);
        coll.update(1, 1);

        assert_eq!(coll.get(&1), 2);
        assert_eq!(coll.get(&2), 1);
        assert_eq!(coll.get(&3), 0);
        assert!(coll.contains(&1));
        assert!(!coll.contains(&3));
    }

    #[test]
    fn test_collection_delete() {
        let mut coll = Multiset::new();
        coll.update(1, 1);
        coll.update(1, 1);
        coll.update(1, -1);

        assert_eq!(coll.get(&1), 1);
        coll.update(1, -1);
        assert!(!coll.contains(&1));
        assert!(coll.is_empty());
    }
}
