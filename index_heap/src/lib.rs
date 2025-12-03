use index_list::{Index, IndexList};
use std::collections::HashMap;
use std::hash::Hash;

struct HeapNode<V> {
    value: V,
    parent: Option<Index>,
    left: Option<Index>,
    right: Option<Index>,
}

pub struct L2Heaps<K, V> {
    nodes: IndexList<HeapNode<V>>,
    roots: HashMap<K, Index>,
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
        !self.roots.contains_key(key)
    }

    pub fn contains(&self, key: &K, value: &V) -> bool {
        self.positions.contains_key(&(key.clone(), value.clone()))
    }

    pub fn peek(&self, key: &K) -> Option<&V> {
        let root_idx = *self.roots.get(key)?;
        Some(&self.nodes.get(root_idx)?.value)
    }

    pub fn push(&mut self, key: K, value: V) -> Index {
        let node = HeapNode {
            value: value.clone(),
            parent: None,
            left: None,
            right: None,
        };
        let idx = self.nodes.insert_last(node);
        self.positions.insert((key.clone(), value), idx);

        if let Some(&root_idx) = self.roots.get(&key) {
            let parent_idx = self.find_insertion_parent(root_idx);
            self.nodes.get_mut(idx).unwrap().parent = Some(parent_idx);
            let parent = self.nodes.get_mut(parent_idx).unwrap();
            if parent.left.is_none() {
                parent.left = Some(idx);
            } else {
                parent.right = Some(idx);
            }
            self.bubble_up(&key, idx);
        } else {
            self.roots.insert(key, idx);
        }
        idx
    }

    pub fn pop(&mut self, key: &K) -> Option<V> {
        let root_idx = *self.roots.get(key)?;
        Some(self.remove_at(key, root_idx))
    }

    pub fn remove(&mut self, key: &K, value: &V) -> bool {
        let Some(&idx) = self.positions.get(&(key.clone(), value.clone())) else {
            return false;
        };
        self.remove_at(key, idx);
        true
    }

    fn remove_at(&mut self, key: &K, idx: Index) -> V {
        let last_idx = self.find_last_node(*self.roots.get(key).unwrap());

        if idx == last_idx {
            return self.remove_leaf(key, idx);
        }

        // Swap values between idx and last
        let last_value = self.remove_leaf(key, last_idx);
        let node = self.nodes.get_mut(idx).unwrap();
        let old_value = std::mem::replace(&mut node.value, last_value.clone());

        self.positions.remove(&(key.clone(), old_value.clone()));
        self.positions.insert((key.clone(), last_value), idx);

        self.reheapify(key, idx);
        old_value
    }

    fn remove_leaf(&mut self, key: &K, idx: Index) -> V {
        let node = self.nodes.get(idx).unwrap();
        let parent_idx = node.parent;
        let value = node.value.clone();

        if let Some(parent_idx) = parent_idx {
            let parent = self.nodes.get_mut(parent_idx).unwrap();
            if parent.left == Some(idx) {
                parent.left = None;
            } else {
                parent.right = None;
            }
        } else {
            self.roots.remove(key);
        }

        self.positions.remove(&(key.clone(), value.clone()));
        self.nodes.remove(idx);
        value
    }

    fn find_insertion_parent(&self, root_idx: Index) -> Index {
        // BFS to find first node with missing child
        let mut queue = vec![root_idx];
        let mut i = 0;
        while i < queue.len() {
            let node = self.nodes.get(queue[i]).unwrap();
            if node.left.is_none() || node.right.is_none() {
                return queue[i];
            }
            if let Some(left) = node.left {
                queue.push(left);
            }
            if let Some(right) = node.right {
                queue.push(right);
            }
            i += 1;
        }
        queue[i]
    }

    fn find_last_node(&self, root_idx: Index) -> Index {
        // BFS to find last node in level order
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

impl<K: Hash + Eq + Clone, V: Ord + Hash + Eq + Clone> Default for L2Heaps<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
