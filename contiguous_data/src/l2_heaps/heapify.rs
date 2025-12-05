//! Heap maintenance operations (bubble up, bubble down, reheapify).

use super::L2Heaps;
use index_list::Index;
use std::hash::Hash;

impl<K: Hash + Eq + Clone, V: Ord + Hash + Eq + Clone, const N: usize> L2Heaps<K, V, N> {
    pub(super) fn reheapify(&mut self, key: &K, idx: Index) {
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

    pub(super) fn bubble_up(&mut self, key: &K, mut idx: Index) {
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

    pub(super) fn bubble_down(&mut self, key: &K, mut idx: Index) {
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
