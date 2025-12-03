//! List operations for L2Multiset (promote, push, remove, demote).

use super::{L2Multiset, ListNode, Root};
use arrayvec::ArrayVec;
use index_list::Index;
use std::hash::Hash;

impl<K: Hash + Eq + Clone, V: Hash + Eq + Clone> L2Multiset<K, V> {
    pub(crate) fn add_to_structure(&mut self, key: K, value: V) {
        match self.roots.get(&key) {
            None => {
                let mut arr = ArrayVec::new();
                arr.push(value);
                self.roots.insert(key, Root::Small(arr));
            }
            Some(Root::Small(arr)) if arr.len() < 2 => {
                self.roots.get_mut(&key).unwrap().as_small_mut().push(value);
            }
            Some(Root::Small(_)) => {
                let arr = std::mem::take(self.roots.get_mut(&key).unwrap().as_small_mut());
                let (head, len) = self.promote_to_large(&key, arr, value);
                *self.roots.get_mut(&key).unwrap() = Root::Large { head, len };
            }
            Some(Root::Large { head, .. }) => {
                let head = *head;
                self.push_large(&key, head, value);
                self.roots.get_mut(&key).unwrap().inc_len();
            }
        }
    }

    pub(crate) fn remove_from_structure(&mut self, key: &K, value: &V) {
        match self.roots.get(key) {
            None => {}
            Some(Root::Small(arr)) => {
                if let Some(pos) = arr.iter().position(|v| v == value) {
                    self.roots.get_mut(key).unwrap().as_small_mut().remove(pos);
                    if self.roots.get(key).unwrap().as_small().is_empty() {
                        self.roots.remove(key);
                    }
                }
            }
            Some(Root::Large { .. }) => {
                if let Some(&idx) = self.positions.get(&(key.clone(), value.clone())) {
                    self.remove_at_large(key, idx);
                    self.maybe_demote(key);
                }
            }
        }
    }

    fn promote_to_large(&mut self, key: &K, arr: ArrayVec<V, 2>, new_value: V) -> (Index, usize) {
        let mut prev_idx: Option<Index> = None;
        let mut head_idx: Option<Index> = None;

        for value in arr {
            let idx = self.nodes.insert_first(ListNode {
                value: value.clone(),
                next: None,
                prev: prev_idx,
            });
            self.positions.insert((key.clone(), value), idx);

            if let Some(prev) = prev_idx {
                self.nodes.get_mut(prev).unwrap().next = Some(idx);
            } else {
                head_idx = Some(idx);
            }
            prev_idx = Some(idx);
        }

        // Add the new value
        let idx = self.nodes.insert_first(ListNode {
            value: new_value.clone(),
            next: None,
            prev: prev_idx,
        });
        self.positions.insert((key.clone(), new_value), idx);
        if let Some(prev) = prev_idx {
            self.nodes.get_mut(prev).unwrap().next = Some(idx);
        } else {
            head_idx = Some(idx);
        }

        (head_idx.unwrap(), 3)
    }

    fn push_large(&mut self, key: &K, head: Index, value: V) {
        let new_idx = self.nodes.insert_first(ListNode {
            value: value.clone(),
            next: Some(head),
            prev: None,
        });
        self.positions.insert((key.clone(), value), new_idx);
        self.nodes.get_mut(head).unwrap().prev = Some(new_idx);

        if let Some(Root::Large { head: h, .. }) = self.roots.get_mut(key) {
            *h = new_idx;
        }
    }

    fn remove_at_large(&mut self, key: &K, idx: Index) -> V {
        let node = self.nodes.remove(idx).unwrap();
        self.positions
            .remove(&(key.clone(), node.value.clone()))
            .unwrap();

        if let Some(prev) = node.prev {
            self.nodes.get_mut(prev).unwrap().next = node.next;
        } else {
            // Was head, update root and new head's prev
            if let Some(Root::Large { head, len, .. }) = self.roots.get_mut(key) {
                *head = node.next.unwrap();
                *len -= 1;
                // Update new head's prev to None
                self.nodes.get_mut(*head).unwrap().prev = None;
            }
            return node.value;
        }
        if let Some(next) = node.next {
            self.nodes.get_mut(next).unwrap().prev = node.prev;
        }

        self.roots.get_mut(key).unwrap().dec_len();
        node.value
    }

    fn maybe_demote(&mut self, key: &K) {
        let Some(Root::Large { head, len }) = self.roots.get(key) else {
            return;
        };
        if *len > 2 {
            return;
        }

        let head = *head;
        let mut arr = ArrayVec::new();
        let mut current = Some(head);
        while let Some(idx) = current {
            let node = self.nodes.remove(idx).unwrap();
            self.positions.remove(&(key.clone(), node.value.clone()));
            current = node.next;
            arr.push(node.value);
        }

        if arr.is_empty() {
            self.roots.remove(key);
        } else {
            *self.roots.get_mut(key).unwrap() = Root::Small(arr);
        }
    }
}
