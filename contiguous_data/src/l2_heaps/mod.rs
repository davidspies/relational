mod heap_ops;
mod heap_root;
mod heapify;
mod mutations;

use arrayvec::ArrayVec;
use index_list::{Index, IndexList};
use std::hash::Hash;

use ahash::AHashMap;

pub(crate) use heap_root::HeapRoot;

pub(crate) struct HeapNode<V> {
    value: V,
    parent: Option<Index>,
    left: Option<Index>,
    right: Option<Index>,
}

pub struct L2Heaps<K, V, const N: usize = 2> {
    nodes: IndexList<HeapNode<V>>,
    roots: AHashMap<K, HeapRoot<V, N>>,
    positions: AHashMap<(K, V), Index>,
    scratch: Vec<Index>,
    scratch_values: Vec<V>,
}

impl<K: Hash + Eq + Clone, V: Ord + Hash + Eq + Clone, const N: usize> L2Heaps<K, V, N> {
    pub fn new() -> Self {
        Self {
            nodes: IndexList::new(),
            roots: AHashMap::new(),
            positions: AHashMap::new(),
            scratch: Vec::new(),
            scratch_values: Vec::new(),
        }
    }

    pub fn is_empty(&self, key: &K) -> bool {
        match self.roots.get(key) {
            None => true,
            Some(HeapRoot::Small(arr)) => arr.is_empty(),
            Some(HeapRoot::Large { .. }) => false,
        }
    }

    pub fn contains(&self, key: &K, value: &V) -> bool {
        match self.roots.get(key) {
            None => false,
            Some(HeapRoot::Small(arr)) => arr.contains(value),
            Some(HeapRoot::Large { top, .. }) => {
                top.contains(value) || self.positions.contains_key(&(key.clone(), value.clone()))
            }
        }
    }

    pub fn peek(&self, key: &K) -> Option<&V> {
        match self.roots.get(key)? {
            HeapRoot::Small(arr) => arr.first(),
            HeapRoot::Large { top, .. } => top.first(),
        }
    }

    /// Get the top N smallest values for a key (or fewer if less than N exist).
    /// Returns None if no values exist for the key.
    pub fn get_top(&self, key: &K) -> Option<&ArrayVec<V, N>> {
        match self.roots.get(key)? {
            HeapRoot::Small(arr) => Some(arr),
            HeapRoot::Large { top, .. } => Some(top),
        }
    }
}

impl<K: Hash + Eq + Clone, V: Ord + Hash + Eq + Clone, const N: usize> Default
    for L2Heaps<K, V, N>
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
