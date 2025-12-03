//! L2Multiset - a HashMap<K, Multiset<V>> with shared contiguous storage.

use arrayvec::ArrayVec;
use index_list::{Index, IndexList};
use std::collections::HashMap;
use std::hash::Hash;

use crate::{Diff, Multiset};

struct ListNode<V> {
    value: V,
    next: Option<Index>,
    prev: Option<Index>,
}

enum Root<V> {
    Small(ArrayVec<V, 2>),
    Large { head: Index, len: usize },
}

pub struct L2Multiset<K, V> {
    nodes: IndexList<ListNode<V>>,
    roots: HashMap<K, Root<V>>,
    positions: HashMap<(K, V), Index>,
    counts: Multiset<(K, V)>,
    /// Keys that "exist" - created on insert, removed when multiset is empty after delete.
    existing_keys: std::collections::HashSet<K>,
}

impl<K: Hash + Eq + Clone, V: Hash + Eq + Clone> L2Multiset<K, V> {
    pub fn new() -> Self {
        Self {
            nodes: IndexList::new(),
            roots: HashMap::new(),
            positions: HashMap::new(),
            counts: Multiset::new(),
            existing_keys: std::collections::HashSet::new(),
        }
    }

    /// Check if key has no values with non-zero count.
    pub fn is_empty(&self, key: &K) -> bool {
        !self.roots.contains_key(key)
    }

    /// Get the count of (key, value).
    pub fn get(&self, key: &K, value: &V) -> Diff {
        self.counts.get(&(key.clone(), value.clone()))
    }

    /// Check if (key, value) exists with non-zero count.
    pub fn contains(&self, key: &K, value: &V) -> bool {
        self.counts.contains(&(key.clone(), value.clone()))
    }

    /// Insert (key, value) - increment count by 1.
    pub fn insert(&mut self, key: K, value: V) {
        // Mark key as existing
        self.existing_keys.insert(key.clone());

        let kv = (key.clone(), value.clone());
        let was_present = self.counts.contains(&kv);
        self.counts.insert(kv);
        let is_present = self.counts.contains(&(key.clone(), value.clone()));

        // Update structure based on presence change
        if !was_present && is_present {
            self.add_to_structure(key, value);
        } else if was_present && !is_present {
            self.remove_from_structure(&key, &value);
        }
    }

    /// Delete (key, value) - decrement count by 1.
    /// No-op if the key doesn't exist (matches HashMap<K, Multiset<V>> semantics).
    pub fn delete(&mut self, key: &K, value: &V) {
        if !self.existing_keys.contains(key) {
            return;
        }
        let kv = (key.clone(), value.clone());
        let was_present = self.counts.contains(&kv);
        self.counts.delete(kv);
        let is_present = self.counts.contains(&(key.clone(), value.clone()));

        // Remove key if multiset is empty (no non-zero counts for this key)
        if self.key_is_empty_in_counts(key) {
            self.existing_keys.remove(key);
        }

        // Update structure based on presence change
        if !was_present && is_present {
            self.add_to_structure(key.clone(), value.clone());
        } else if was_present && !is_present {
            self.remove_from_structure(key, value);
        }
    }

    /// Check if a key has no entries in counts (all counts are zero).
    fn key_is_empty_in_counts(&self, key: &K) -> bool {
        // This is O(n) in the number of distinct values, but it's only called
        // after delete when we need to check if the key should be removed.
        // A more efficient approach would track this separately.
        !self
            .counts
            .iter_with_multiplicity()
            .any(|((k, _), _)| k == key)
    }

    /// Iterate over values for a key (each value appears once regardless of count).
    pub fn iter_values(&self, key: &K) -> impl Iterator<Item = &V> {
        let root = self.roots.get(key);
        let nodes = &self.nodes;
        L2MultisetIter {
            nodes,
            state: match root {
                None => IterState::Done,
                Some(Root::Small(arr)) => IterState::Small(arr.iter()),
                Some(Root::Large { head, .. }) => IterState::Large(Some(*head)),
            },
        }
    }

    /// Get the number of distinct values for a key.
    pub fn len(&self, key: &K) -> usize {
        match self.roots.get(key) {
            None => 0,
            Some(Root::Small(arr)) => arr.len(),
            Some(Root::Large { len, .. }) => *len,
        }
    }
}

impl<V> Root<V> {
    fn as_small(&self) -> &ArrayVec<V, 2> {
        match self {
            Root::Small(arr) => arr,
            Root::Large { .. } => panic!("expected Small"),
        }
    }

    fn as_small_mut(&mut self) -> &mut ArrayVec<V, 2> {
        match self {
            Root::Small(arr) => arr,
            Root::Large { .. } => panic!("expected Small"),
        }
    }

    fn inc_len(&mut self) {
        match self {
            Root::Large { len, .. } => *len += 1,
            Root::Small(_) => panic!("expected Large"),
        }
    }

    fn dec_len(&mut self) {
        match self {
            Root::Large { len, .. } => *len -= 1,
            Root::Small(_) => panic!("expected Large"),
        }
    }
}

impl<K: Hash + Eq + Clone, V: Hash + Eq + Clone> Default for L2Multiset<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

enum IterState<'a, V> {
    Small(std::slice::Iter<'a, V>),
    Large(Option<Index>),
    Done,
}

struct L2MultisetIter<'a, V> {
    nodes: &'a IndexList<ListNode<V>>,
    state: IterState<'a, V>,
}

impl<'a, V> Iterator for L2MultisetIter<'a, V> {
    type Item = &'a V;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.state {
            IterState::Small(iter) => iter.next(),
            IterState::Large(current) => {
                let idx = (*current)?;
                let node = self.nodes.get(idx)?;
                *current = node.next;
                Some(&node.value)
            }
            IterState::Done => None,
        }
    }
}

mod list_ops;

#[cfg(test)]
mod tests;
