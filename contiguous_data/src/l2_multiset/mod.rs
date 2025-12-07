//! L2Multiset - a HashMap<K, Multiset<V>> with shared contiguous storage.
mod list_ops;
mod root;

use std::hash::Hash;

use derive_where::derive_where;
use index_list::{Index, IndexList};

use crate::{Diff, HashMap, Multiset};

use self::root::Root;

struct ListNode<V> {
    value: V,
    next: Option<Index>,
    prev: Option<Index>,
}

#[derive_where(Default)]
pub struct L2Multiset<K, V> {
    nodes: IndexList<ListNode<V>>,
    roots: HashMap<K, Root<V>>,
    positions: HashMap<(K, V), Index>,
    counts: Multiset<(K, V)>,
}

impl<K: Hash + Eq + Clone, V: Hash + Eq + Clone> L2Multiset<K, V> {
    pub fn new() -> Self {
        Self::default()
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

#[cfg(test)]
mod tests;
