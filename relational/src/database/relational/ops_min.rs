//! Min operator - minimum N values by key.

use std::hash::Hash;

use arrayvec::ArrayVec;
use contiguous_data::{Diff, L2Heaps, Multiset};

use super::op::Op;
use super::relation::Relation;

/// A min-N operator - tracks the N smallest values by key.
/// Input is (key, value) pairs, output is (key, ArrayVec<value, N>) pairs.
pub struct MinNOp<K, V, R, const N: usize>
where
    K: Eq + Hash,
    V: Ord + Hash + Eq,
    R: Op<(K, V)>,
{
    inner: Relation<R>,
    /// Track all values per key - heap gives us efficient top-N lookup
    heap: L2Heaps<K, V, N>,
    /// Track counts for each (key, value) pair
    counts: Multiset<(K, V)>,
}

impl<K, V, R, const N: usize> Op<(K, ArrayVec<V, N>)> for MinNOp<K, V, R, N>
where
    K: Clone + Eq + Hash,
    V: Clone + Ord + Hash + Eq,
    R: Op<(K, V)>,
{
    fn foreach(&mut self, mut consumer: impl FnMut((K, ArrayVec<V, N>), Diff)) {
        let heap = &mut self.heap;
        let counts = &mut self.counts;

        self.inner.foreach(|(k, v), diff| {
            // Get old top-N before update
            let old_top = heap.get_top(&k).cloned();

            // Get old and new counts
            let old_count = counts.get(&(k.clone(), v.clone()));
            counts.update((k.clone(), v.clone()), diff);
            let new_count = counts.get(&(k.clone(), v.clone()));

            // Update heap based on count transitions (present when count != 0)
            if old_count == 0 && new_count != 0 {
                heap.push(k.clone(), v.clone());
            } else if old_count != 0 && new_count == 0 {
                heap.remove(&k, &v);
            }

            // Get new top-N after update
            let new_top = heap.get_top(&k).cloned();

            // Output changes if top-N changed
            if old_top != new_top {
                if let Some(old) = old_top {
                    consumer((k.clone(), old), -1);
                }
                if let Some(new) = new_top {
                    consumer((k, new), 1);
                }
            }
        });
    }
}

impl<R> Relation<R> {
    /// Top N smallest values by key (without consolidation).
    /// Input must be (K, V) tuples where K is the key and V is the value.
    /// Output is (K, ArrayVec<V, N>) where the ArrayVec contains up to N smallest values.
    pub fn group_min_n_<K, V, const N: usize>(self) -> Relation<impl Op<(K, ArrayVec<V, N>)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash + Ord,
    {
        let node_id = self.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        Relation::new(
            MinNOp {
                inner: self,
                heap: L2Heaps::new(),
                counts: Multiset::new(),
            },
            commit_id,
            graph,
            "group_min_n",
            vec![node_id],
        )
    }

    /// Top N smallest values by key.
    /// Input must be (K, V) tuples where K is the key and V is the value.
    /// Output is (K, ArrayVec<V, N>) where the ArrayVec contains up to N smallest values.
    pub fn group_min_n<K, V, const N: usize>(self) -> Relation<impl Op<(K, ArrayVec<V, N>)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash + Ord,
    {
        let result = self.group_min_n_();
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_passthrough_();
        result
    }
}
