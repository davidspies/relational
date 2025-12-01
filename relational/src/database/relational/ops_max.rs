//! Max operator - maximum value by key.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

use crate::change::Diff;

use super::relation::{Op, Relation};

/// A max operator - tracks maximum value by key.
/// Input is (key, value) pairs, output is (key, max_value) pairs.
pub struct MaxOp<K, V, R>
where
    K: Eq + Hash,
    V: Ord,
    R: Op<(K, V)>,
{
    inner: Relation<R>,
    /// Track all values per key with their counts: key -> (value -> count)
    /// Using BTreeMap so we can efficiently find max
    values: HashMap<K, BTreeMap<V, i64>>,
}

impl<K, V, R> Op<(K, V)> for MaxOp<K, V, R>
where
    K: Clone + Eq + Hash,
    V: Clone + Ord,
    R: Op<(K, V)>,
{
    fn foreach(&mut self, mut consumer: impl FnMut((K, V), Diff)) {
        let values = &mut self.values;

        self.inner.foreach(|(k, v), diff| {
            let key_values = values.entry(k.clone()).or_default();

            // Get old max before update
            let old_max = key_values.keys().next_back().cloned();

            // Update the value count
            let count = key_values.entry(v.clone()).or_insert(0);
            *count += diff;
            if *count == 0 {
                key_values.remove(&v);
            }

            // Clean up empty key entries
            if key_values.is_empty() {
                values.remove(&k);
            }

            // Get new max after update
            let new_max = values.get(&k).and_then(|kv| kv.keys().next_back().cloned());

            // Output changes if max changed
            if old_max != new_max {
                if let Some(old) = old_max {
                    consumer((k.clone(), old), -1);
                }
                if let Some(new) = new_max {
                    consumer((k, new), 1);
                }
            }
        });
    }
}

impl<R> Relation<R> {
    /// Maximum value by key (without consolidation).
    /// Input must be (K, V) tuples where K is the key and V is the value.
    pub fn group_max_<K, V>(self) -> Relation<impl Op<(K, V)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash + Ord,
    {
        let node_id = self.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        Relation::new(
            MaxOp {
                inner: self,
                values: HashMap::new(),
            },
            commit_id,
            graph,
            "max",
            vec![node_id],
        )
    }

    /// Maximum value by key.
    /// Input must be (K, V) tuples where K is the key and V is the value.
    pub fn group_max<K, V>(self) -> Relation<impl Op<(K, V)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash + Ord,
    {
        let result = self.group_max_();
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_();
        result
    }
}
