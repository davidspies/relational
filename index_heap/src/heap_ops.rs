use super::{HeapNode, HeapRoot, L2Heaps};
use arrayvec::ArrayVec;
use index_list::Index;
use std::hash::Hash;

impl<K: Hash + Eq + Clone, V: Ord + Hash + Eq + Clone> L2Heaps<K, V> {
    pub(crate) fn promote_to_large(
        &mut self,
        key: &K,
        arr: ArrayVec<V, 2>,
        new_value: V,
    ) -> (Index, usize) {
        let mut all_values: ArrayVec<V, 3> = arr.into_iter().collect();
        all_values.push(new_value);
        all_values.sort();

        let root_node = HeapNode {
            value: all_values[0].clone(),
            parent: None,
            left: None,
            right: None,
        };
        let root_idx = self.nodes.insert_last(root_node);
        self.positions
            .insert((key.clone(), all_values[0].clone()), root_idx);

        let left_node = HeapNode {
            value: all_values[1].clone(),
            parent: Some(root_idx),
            left: None,
            right: None,
        };
        let left_idx = self.nodes.insert_last(left_node);
        self.positions
            .insert((key.clone(), all_values[1].clone()), left_idx);
        self.nodes.get_mut(root_idx).unwrap().left = Some(left_idx);

        let right_node = HeapNode {
            value: all_values[2].clone(),
            parent: Some(root_idx),
            left: None,
            right: None,
        };
        let right_idx = self.nodes.insert_last(right_node);
        self.positions
            .insert((key.clone(), all_values[2].clone()), right_idx);
        self.nodes.get_mut(root_idx).unwrap().right = Some(right_idx);

        (root_idx, 3)
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
            Some(HeapRoot::Large { size, .. }) if *size <= 2
        );

        if should_demote {
            if let Some(HeapRoot::Large { root, size }) = self.roots.remove(key) {
                let values = self.extract_and_remove_all(key, root, size);
                let arr: ArrayVec<V, 2> = values.into_iter().collect();
                // arr is never empty: we only demote when size is 1 or 2
                self.roots.insert(key.clone(), HeapRoot::Small(arr));
            }
        }
    }

    fn extract_and_remove_all(&mut self, key: &K, root: Index, size: usize) -> Vec<V> {
        let mut values = Vec::with_capacity(size);
        let mut queue = vec![root];
        let mut i = 0;
        while i < queue.len() {
            let node = self.nodes.get(queue[i]).unwrap();
            values.push(node.value.clone());
            if let Some(left) = node.left {
                queue.push(left);
            }
            if let Some(right) = node.right {
                queue.push(right);
            }
            i += 1;
        }
        for idx in queue {
            let value = &self.nodes.get(idx).unwrap().value;
            self.positions.remove(&(key.clone(), value.clone()));
            self.nodes.remove(idx);
        }
        values.sort();
        values
    }

    pub(crate) fn remove_at_large(&mut self, key: &K, idx: Index) -> V {
        let root_idx = match self.roots.get(key).unwrap() {
            HeapRoot::Large { root, .. } => *root,
            HeapRoot::Small(_) => panic!("expected Large"),
        };
        let last_idx = self.find_last_node(root_idx);

        match self.roots.get_mut(key).unwrap() {
            HeapRoot::Large { size, .. } => *size -= 1,
            HeapRoot::Small(_) => panic!("expected Large"),
        }

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
        // Parent is always Some: we only remove leaves, and Large heaps have size >= 3,
        // so there's always a parent node.
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

    fn find_insertion_parent(&self, root_idx: Index) -> Index {
        let mut queue = vec![root_idx];
        let mut i = 0;
        while i < queue.len() {
            let node = self.nodes.get(queue[i]).unwrap();
            if node.left.is_none() || node.right.is_none() {
                return queue[i];
            }
            queue.push(node.left.unwrap());
            queue.push(node.right.unwrap());
            i += 1;
        }
        unreachable!("heap always has a node with missing child")
    }

    fn find_last_node(&self, root_idx: Index) -> Index {
        let mut queue = vec![root_idx];
        let mut i = 0;
        while i < queue.len() {
            let node = self.nodes.get(queue[i]).unwrap();
            if let Some(left) = node.left {
                queue.push(left);
            }
            if let Some(right) = node.right {
                queue.push(right);
            }
            i += 1;
        }
        queue[queue.len() - 1]
    }

    fn reheapify(&mut self, key: &K, idx: Index) {
        let node = self.nodes.get(idx).unwrap();
        if let Some(parent_idx) = node.parent {
            let parent = self.nodes.get(parent_idx).unwrap();
            if node.value < parent.value {
                self.bubble_up(key, idx);
                return;
            }
        }
        self.bubble_down(key, idx);
    }

    fn bubble_up(&mut self, key: &K, mut idx: Index) {
        while let Some(parent_idx) = self.nodes.get(idx).unwrap().parent {
            let node_val = &self.nodes.get(idx).unwrap().value;
            let parent_val = &self.nodes.get(parent_idx).unwrap().value;
            if node_val >= parent_val {
                break;
            }
            self.swap_values(key, idx, parent_idx);
            idx = parent_idx;
        }
    }

    fn bubble_down(&mut self, key: &K, mut idx: Index) {
        loop {
            let node = self.nodes.get(idx).unwrap();
            let mut smallest = idx;
            let mut smallest_val = &node.value;

            if let Some(left_idx) = node.left {
                let left_val = &self.nodes.get(left_idx).unwrap().value;
                if left_val < smallest_val {
                    smallest = left_idx;
                    smallest_val = left_val;
                }
            }

            let node = self.nodes.get(idx).unwrap();
            if let Some(right_idx) = node.right {
                let right_val = &self.nodes.get(right_idx).unwrap().value;
                if right_val < smallest_val {
                    smallest = right_idx;
                }
            }

            if smallest == idx {
                break;
            }
            self.swap_values(key, idx, smallest);
            idx = smallest;
        }
    }

    fn swap_values(&mut self, key: &K, i: Index, j: Index) {
        let val_i = self.nodes.get(i).unwrap().value.clone();
        let val_j = self.nodes.get(j).unwrap().value.clone();

        self.nodes.get_mut(i).unwrap().value = val_j.clone();
        self.nodes.get_mut(j).unwrap().value = val_i.clone();

        self.positions.insert((key.clone(), val_i), j);
        self.positions.insert((key.clone(), val_j), i);
    }
}
