//! L2Multiset - a AHashMap<K, Multiset<V>> with shared contiguous storage.

use std::hash::Hash;

use arrayvec::ArrayVec;
use index_list::{Index, IndexList};

use ahash::AHashMap;

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
    roots: AHashMap<K, Root<V>>,
    positions: AHashMap<(K, V), Index>,
    counts: Multiset<(K, V)>,
}

impl<K: Hash + Eq + Clone, V: Hash + Eq + Clone> L2Multiset<K, V> {
    pub fn new() -> Self {
        Self {
            nodes: IndexList::new(),
            roots: AHashMap::new(),
            positions: AHashMap::new(),
            counts: Multiset::new(),
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
    /// No-op if the key doesn't exist (matches AHashMap<K, Multiset<V>> semantics).
    pub fn delete(&mut self, key: &K, value: &V) {
        if !self.roots.contains_key(key) {
            return;
        }
        let kv = (key.clone(), value.clone());
        let was_present = self.counts.contains(&kv);
        self.counts.delete(kv);
        let is_present = self.counts.contains(&(key.clone(), value.clone()));

        // Update structure based on presence change
        if !was_present && is_present {
            self.add_to_structure(key.clone(), value.clone());
        } else if was_present && !is_present {
            self.remove_from_structure(key, value);
        }
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

    /// Update (key, value) by an arbitrary diff.
    pub fn update(&mut self, key: K, value: V, diff: Diff) {
        if diff == 0 {
            return;
        }

        let kv = (key.clone(), value.clone());
        let was_present = self.counts.contains(&kv);
        self.counts.update(kv, diff);
        let is_present = self.counts.contains(&(key.clone(), value.clone()));

        // Update structure based on presence change
        if !was_present && is_present {
            self.add_to_structure(key, value);
        } else if was_present && !is_present {
            self.remove_from_structure(&key, &value);
        }
    }

    /// Iterate over (value, count) pairs for a key.
    pub fn iter_with_multiplicity<'a>(
        &'a self,
        key: &'a K,
    ) -> impl Iterator<Item = (&'a V, Diff)> + 'a {
        self.iter_values(key)
            .map(move |v| (v, self.counts.get(&(key.clone(), v.clone()))))
    }

    /// Get the value for the key if there is at most one value.
    /// Panic if there is more than one.
    #[track_caller]
    pub fn get_singleton(&self, key: &K) -> Option<&V> {
        let mut iter = self.iter_values(key);
        let output = iter.next()?;
        assert!(iter.next().is_none(), "Expected singleton for key");
        Some(output)
    }

    /// Iterate over all keys that have at least one value.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.roots.keys()
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
