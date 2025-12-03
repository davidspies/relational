//! Min operator - minimum value by key.

use std::hash::Hash;

use contiguous_data::L2Heaps;

use contiguous_data::{Diff, Multiset};

use super::relation::{Op, Relation};

/// A min operator - tracks minimum value by key.
/// Input is (key, value) pairs, output is (key, min_value) pairs.
pub struct MinOp<K, V, R>
where
    K: Eq + Hash,
    V: Ord + Hash + Eq,
    R: Op<(K, V)>,
{
    inner: Relation<R>,
    /// Track all values per key - heap gives us efficient min lookup
    heap: L2Heaps<K, V>,
    /// Track counts for each (key, value) pair
    counts: Multiset<(K, V)>,
}

impl<K, V, R> Op<(K, V)> for MinOp<K, V, R>
where
    K: Clone + Eq + Hash,
    V: Clone + Ord + Hash + Eq,
    R: Op<(K, V)>,
{
    fn foreach(&mut self, mut consumer: impl FnMut((K, V), Diff)) {
        let heap = &mut self.heap;
        let counts = &mut self.counts;

        self.inner.foreach(|(k, v), diff| {
            // Get old min before update
            let old_min = heap.peek(&k).cloned();

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

            // Get new min after update
            let new_min = heap.peek(&k).cloned();

            // Output changes if min changed
            if old_min != new_min {
                if let Some(old) = old_min {
                    consumer((k.clone(), old), -1);
                }
                if let Some(new) = new_min {
                    consumer((k, new), 1);
                }
            }
        });
    }
}

impl<R> Relation<R> {
    /// Minimum value by key (without consolidation).
    /// Input must be (K, V) tuples where K is the key and V is the value.
    pub fn group_min_<K, V>(self) -> Relation<impl Op<(K, V)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash + Ord,
    {
        let node_id = self.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        Relation::new(
            MinOp {
                inner: self,
                heap: L2Heaps::new(),
                counts: Multiset::new(),
            },
            commit_id,
            graph,
            "group_min",
            vec![node_id],
        )
    }

    /// Minimum value by key.
    /// Input must be (K, V) tuples where K is the key and V is the value.
    pub fn group_min<K, V>(self) -> Relation<impl Op<(K, V)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash + Ord,
    {
        let result = self.group_min_();
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_passthrough_();
        result
    }
}
