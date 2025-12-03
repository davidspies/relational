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

enum HeapRoot<V, const N: usize> {
    /// 0 to N elements, kept sorted.
    Small(ArrayVec<V, N>),
    /// N elements in top (sorted), plus overflow in heap.
    Large {
        top: ArrayVec<V, N>,
        root: Index,
        heap_size: usize,
    },
}

pub struct L2Heaps<K, V, const N: usize = 2> {
    nodes: IndexList<HeapNode<V>>,
    roots: HashMap<K, HeapRoot<V, N>>,
    positions: HashMap<(K, V), Index>,
}

impl<K: Hash + Eq + Clone, V: Ord + Hash + Eq + Clone, const N: usize> L2Heaps<K, V, N> {
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

    pub fn push(&mut self, key: K, value: V) {
        match self.roots.get(&key) {
            None => {
                let mut arr = ArrayVec::new();
                arr.push(value);
                self.roots.insert(key, HeapRoot::Small(arr));
            }
            Some(HeapRoot::Small(arr)) if arr.len() < N => {
                let pos = arr.iter().position(|v| &value < v).unwrap_or(arr.len());
                self.roots
                    .get_mut(&key)
                    .unwrap()
                    .as_small_mut()
                    .insert(pos, value);
            }
            Some(HeapRoot::Small(_)) => {
                // Small is full (N elements), need to promote to Large
                let arr = std::mem::replace(
                    self.roots.get_mut(&key).unwrap().as_small_mut(),
                    ArrayVec::new(),
                );
                let (top, root) = self.promote_to_large(&key, arr, value);
                *self.roots.get_mut(&key).unwrap() = HeapRoot::Large { top, root, heap_size: 1 };
            }
            Some(HeapRoot::Large { top, root, .. }) => {
                let root = *root;
                // If value belongs in top N, insert there and push displaced to heap
                let last_top = top.last().unwrap();
                if &value < last_top {
                    let pos = top.iter().position(|v| &value < v).unwrap();
                    let top = self.roots.get_mut(&key).unwrap().as_top_mut();
                    let displaced = top.pop().unwrap();
                    top.insert(pos, value);
                    self.push_large(&key, root, displaced);
                } else {
                    self.push_large(&key, root, value);
                }
                self.roots.get_mut(&key).unwrap().inc_heap_size();
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
                // Remove from top and pull from heap
                let value = self.roots.get_mut(key).unwrap().as_top_mut().remove(0);
                let replacement = self.remove_at_large(key, root_idx);
                let pos = {
                    let top = self.roots.get(key).unwrap().as_top();
                    top.iter().position(|v| &replacement < v).unwrap_or(top.len())
                };
                self.roots.get_mut(key).unwrap().as_top_mut().insert(pos, replacement);
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
            Some(HeapRoot::Large { top, root, .. }) => {
                let root_idx = *root;
                if let Some(pos) = top.iter().position(|v| v == value) {
                    // Remove from top and pull replacement from heap
                    self.roots.get_mut(key).unwrap().as_top_mut().remove(pos);
                    let replacement = self.remove_at_large(key, root_idx);
                    let pos = {
                        let top = self.roots.get(key).unwrap().as_top();
                        top.iter().position(|v| &replacement < v).unwrap_or(top.len())
                    };
                    self.roots.get_mut(key).unwrap().as_top_mut().insert(pos, replacement);
                    self.maybe_demote(key);
                    true
                } else if let Some(&idx) = self.positions.get(&(key.clone(), value.clone())) {
                    self.remove_at_large(key, idx);
                    self.maybe_demote(key);
                    true
                } else {
                    false
                }
            }
        }
    }
}

impl<V, const N: usize> HeapRoot<V, N> {
    fn as_small(&self) -> &ArrayVec<V, N> {
        match self {
            HeapRoot::Small(arr) => arr,
            HeapRoot::Large { .. } => panic!("expected Small"),
        }
    }

    fn as_small_mut(&mut self) -> &mut ArrayVec<V, N> {
        match self {
            HeapRoot::Small(arr) => arr,
            HeapRoot::Large { .. } => panic!("expected Small"),
        }
    }

    fn as_top(&self) -> &ArrayVec<V, N> {
        match self {
            HeapRoot::Large { top, .. } => top,
            HeapRoot::Small(_) => panic!("expected Large"),
        }
    }

    fn as_top_mut(&mut self) -> &mut ArrayVec<V, N> {
        match self {
            HeapRoot::Large { top, .. } => top,
            HeapRoot::Small(_) => panic!("expected Large"),
        }
    }

    fn inc_heap_size(&mut self) {
        match self {
            HeapRoot::Large { heap_size, .. } => *heap_size += 1,
            HeapRoot::Small(_) => panic!("expected Large"),
        }
    }
}

impl<K: Hash + Eq + Clone, V: Ord + Hash + Eq + Clone, const N: usize> Default for L2Heaps<K, V, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

mod heap_ops;
