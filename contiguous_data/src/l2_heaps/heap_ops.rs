use std::hash::Hash;

use arrayvec::ArrayVec;
use index_list::Index;

use super::heap_root::HeapRoot;
use super::{HeapNode, L2Heaps};

impl<K: Hash + Eq + Clone, V: Ord + Hash + Eq + Clone, const N: usize> L2Heaps<K, V, N> {
    /// Promote from Small (N elements) to Large by adding one more element.
    /// Returns (top N elements sorted, root index of heap with 1 element).
    pub(crate) fn promote_to_large(
        &mut self,
        key: &K,
        arr: ArrayVec<V, N>,
        new_value: V,
    ) -> (ArrayVec<V, N>, Index) {
        // Collect all N+1 values and sort
        assert!(self.scratch_values.is_empty());
        self.scratch_values.extend(arr);
        self.scratch_values.push(new_value);
        self.scratch_values.sort();

        // Last element goes to heap, first N stay in top
        let heap_value = self.scratch_values.pop().unwrap();
        let top: ArrayVec<V, N> = self.scratch_values.drain(..).collect();

        // Create single-node heap
        let root_node = HeapNode {
            value: heap_value.clone(),
            parent: None,
            left: None,
            right: None,
        };
        let root_idx = self.nodes.insert_last(root_node);
        self.positions.insert((key.clone(), heap_value), root_idx);

        (top, root_idx)
    }

    pub(crate) fn push_large(&mut self, key: &K, root_idx: Index, value: V) {
        let node = HeapNode {
            value: value.clone(),
            parent: None,
            left: None,
            right: None,
        };
        let idx = self.nodes.insert_last(node);
        self.positions.insert((key.clone(), value), idx);

        let parent_idx = self.find_insertion_parent(root_idx);
        self.nodes.get_mut(idx).unwrap().parent = Some(parent_idx);
        let parent = self.nodes.get_mut(parent_idx).unwrap();
        if parent.left.is_none() {
            parent.left = Some(idx);
        } else {
            parent.right = Some(idx);
        }
        self.bubble_up(key, idx);
    }

    pub(crate) fn maybe_demote(&mut self, key: &K) {
        let should_demote = matches!(
            self.roots.get(key),
            Some(HeapRoot::Large { heap_size: 0, .. })
        );

        if should_demote && let Some(HeapRoot::Large { top, .. }) = self.roots.remove(key) {
            self.roots.insert(key.clone(), HeapRoot::Small(top));
        }
    }

    /// Remove a node from the heap portion and return its value.
    /// Decrements heap_size.
    pub(crate) fn remove_at_large(&mut self, key: &K, idx: Index) -> V {
        let root_idx = match self.roots.get(key).unwrap() {
            HeapRoot::Large { root, .. } => *root,
            HeapRoot::Small(_) => panic!("expected Large"),
        };

        match self.roots.get_mut(key).unwrap() {
            HeapRoot::Large { heap_size, .. } => *heap_size -= 1,
            HeapRoot::Small(_) => panic!("expected Large"),
        }

        let heap_size = match self.roots.get(key).unwrap() {
            HeapRoot::Large { heap_size, .. } => *heap_size,
            HeapRoot::Small(_) => panic!("expected Large"),
        };

        // If this was the last node in heap, just remove it
        if heap_size == 0 {
            let value = self.nodes.get(idx).unwrap().value.clone();
            self.positions.remove(&(key.clone(), value.clone()));
            self.nodes.remove(idx);
            return value;
        }

        let last_idx = self.find_last_node(root_idx);

        if idx == last_idx {
            return self.remove_leaf_large(key, idx);
        }

        let last_value = self.remove_leaf_large(key, last_idx);
        let node = self.nodes.get_mut(idx).unwrap();
        let old_value = std::mem::replace(&mut node.value, last_value.clone());

        self.positions.remove(&(key.clone(), old_value.clone()));
        self.positions.insert((key.clone(), last_value), idx);

        self.reheapify(key, idx);
        old_value
    }

    fn remove_leaf_large(&mut self, key: &K, idx: Index) -> V {
        let node = self.nodes.get(idx).unwrap();
        // Parent is always Some: we only call this when heap_size > 0 after decrement,
        // meaning at least 2 nodes in heap, so the leaf has a parent.
        let parent_idx = node.parent.unwrap();
        let value = node.value.clone();

        let parent = self.nodes.get_mut(parent_idx).unwrap();
        if parent.left == Some(idx) {
            parent.left = None;
        } else {
            parent.right = None;
        }

        self.positions.remove(&(key.clone(), value.clone()));
        self.nodes.remove(idx);
        value
    }

    fn find_insertion_parent(&mut self, root_idx: Index) -> Index {
        assert!(self.scratch.is_empty());
        self.scratch.push(root_idx);
        let mut i = 0;
        while i < self.scratch.len() {
            let node = self.nodes.get(self.scratch[i]).unwrap();
            if node.left.is_none() || node.right.is_none() {
                let result = self.scratch[i];
                self.scratch.clear();
                return result;
            }
            self.scratch.push(node.left.unwrap());
            self.scratch.push(node.right.unwrap());
            i += 1;
        }
        unreachable!("heap always has a node with missing child")
    }

    fn find_last_node(&mut self, root_idx: Index) -> Index {
        assert!(self.scratch.is_empty());
        self.scratch.push(root_idx);
        let mut i = 0;
        while i < self.scratch.len() {
            let node = self.nodes.get(self.scratch[i]).unwrap();
            if let Some(left) = node.left {
                self.scratch.push(left);
            }
            if let Some(right) = node.right {
                self.scratch.push(right);
            }
            i += 1;
        }
        let result = self.scratch.last().cloned().unwrap();
        self.scratch.clear();
        result
    }
}
