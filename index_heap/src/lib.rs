use arrayvec::ArrayVec;
use index_list::{Index, IndexList};
use std::collections::HashMap;
use std::hash::Hash;

struct HeapNode<V> {
    value: V,
    parent: Option<Index>,
    left: Option<Index>,
    right: Option<Index>,
}

enum HeapRoot<V> {
    Small(ArrayVec<V, 2>),
    Large { root: Index, size: usize },
}

pub struct L2Heaps<K, V> {
    nodes: IndexList<HeapNode<V>>,
    roots: HashMap<K, HeapRoot<V>>,
    positions: HashMap<(K, V), Index>,
}

impl<K: Hash + Eq + Clone, V: Ord + Hash + Eq + Clone> L2Heaps<K, V> {
    pub fn new() -> Self {
        Self {
            nodes: IndexList::new(),
            roots: HashMap::new(),
            positions: HashMap::new(),
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
            Some(HeapRoot::Large { .. }) => {
                self.positions.contains_key(&(key.clone(), value.clone()))
            }
        }
    }

    pub fn peek(&self, key: &K) -> Option<&V> {
        match self.roots.get(key)? {
            HeapRoot::Small(arr) => arr.first(),
            HeapRoot::Large { root, .. } => Some(&self.nodes.get(*root).unwrap().value),
        }
    }

    pub fn push(&mut self, key: K, value: V) {
        match self.roots.get(&key) {
            None => {
                let mut arr = ArrayVec::new();
                arr.push(value);
                self.roots.insert(key, HeapRoot::Small(arr));
            }
            Some(HeapRoot::Small(arr)) if arr.len() < 2 => {
                let pos = arr.iter().position(|v| &value < v).unwrap_or(arr.len());
                self.roots.get_mut(&key).unwrap().as_small_mut().insert(pos, value);
            }
            Some(HeapRoot::Small(_)) => {
                let arr = std::mem::replace(
                    self.roots.get_mut(&key).unwrap().as_small_mut(),
                    ArrayVec::new(),
                );
                let (root, size) = self.promote_to_large(&key, arr, value);
                *self.roots.get_mut(&key).unwrap() = HeapRoot::Large { root, size };
            }
            Some(HeapRoot::Large { root, .. }) => {
                let root = *root;
                self.push_large(&key, root, value);
                self.roots.get_mut(&key).unwrap().inc_size();
            }
        }
    }

    pub fn pop(&mut self, key: &K) -> Option<V> {
        match self.roots.get(key)? {
            HeapRoot::Small(_) => {
                let value = self.roots.get_mut(key).unwrap().as_small_mut().remove(0);
                if self.roots.get(key).unwrap().as_small().is_empty() {
                    self.roots.remove(key);
                }
                Some(value)
            }
            HeapRoot::Large { root, .. } => {
                let root_idx = *root;
                let value = self.remove_at_large(key, root_idx);
                self.maybe_demote(key);
                Some(value)
            }
        }
    }

    pub fn remove(&mut self, key: &K, value: &V) -> bool {
        match self.roots.get(key) {
            None => false,
            Some(HeapRoot::Small(arr)) => {
                if let Some(pos) = arr.iter().position(|v| v == value) {
                    self.roots.get_mut(key).unwrap().as_small_mut().remove(pos);
                    if self.roots.get(key).unwrap().as_small().is_empty() {
                        self.roots.remove(key);
                    }
                    true
                } else {
                    false
                }
            }
            Some(HeapRoot::Large { .. }) => {
                let Some(&idx) = self.positions.get(&(key.clone(), value.clone())) else {
                    return false;
                };
                self.remove_at_large(key, idx);
                self.maybe_demote(key);
                true
            }
        }
    }
}

impl<V> HeapRoot<V> {
    fn as_small(&self) -> &ArrayVec<V, 2> {
        match self {
            HeapRoot::Small(arr) => arr,
            HeapRoot::Large { .. } => panic!("expected Small"),
        }
    }

    fn as_small_mut(&mut self) -> &mut ArrayVec<V, 2> {
        match self {
            HeapRoot::Small(arr) => arr,
            HeapRoot::Large { .. } => panic!("expected Small"),
        }
    }

    fn inc_size(&mut self) {
        match self {
            HeapRoot::Large { size, .. } => *size += 1,
            HeapRoot::Small(_) => panic!("expected Large"),
        }
    }
}

impl<K: Hash + Eq + Clone, V: Ord + Hash + Eq + Clone> Default for L2Heaps<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

mod heap_ops;
