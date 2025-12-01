//! Sum operator - sum values by key.

use std::collections::HashMap;
use std::hash::Hash;
use std::ops::{Add, Mul, Sub};

use crate::change::Diff;

use super::relation::{Op, Relation};

/// A sum operator - sums values by key.
/// Input is (key, value) pairs, output is (key, sum) pairs.
pub struct SumOp<K, V, R>
where
    K: Eq + Hash,
    V: Add<Output = V> + Sub<Output = V> + Mul<i64, Output = V> + Default + PartialEq,
    R: Op<(K, V)>,
{
    inner: Relation<R>,
    /// Track sum per key
    sums: HashMap<K, V>,
}

impl<K, V, R> Op<(K, V)> for SumOp<K, V, R>
where
    K: Clone + Eq + Hash,
    V: Clone + Add<Output = V> + Sub<Output = V> + Mul<i64, Output = V> + Default + PartialEq,
    R: Op<(K, V)>,
{
    fn foreach(&mut self, mut consumer: impl FnMut((K, V), Diff)) {
        let sums = &mut self.sums;

        self.inner.foreach(|(k, v), diff| {
            let delta = v * diff;

            let old_sum = sums.get(&k).cloned().unwrap_or_default();
            let new_sum = old_sum.clone() + delta;

            let zero = V::default();
            if new_sum == zero {
                sums.remove(&k);
            } else {
                sums.insert(k.clone(), new_sum.clone());
            }

            // Output: delete old (key, sum), insert new (key, sum)
            if old_sum != zero {
                consumer((k.clone(), old_sum), -1);
            }
            if new_sum != zero {
                consumer((k, new_sum), 1);
            }
        });
    }
}

impl<R> Relation<R> {
    /// Sum values by key.
    /// Input must be (K, V) tuples where K is the key and V is the value.
    pub fn group_sum<K, V>(self) -> Relation<SumOp<K, V, R>>
    where
        R: Op<(K, V)>,
        K: Eq + Hash,
        V: Add<Output = V> + Sub<Output = V> + Mul<i64, Output = V> + Default + PartialEq,
    {
        let node_id = self.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        Relation::new(
            SumOp {
                inner: self,
                sums: HashMap::new(),
            },
            commit_id,
            graph,
            "sum",
            vec![node_id],
        )
    }
}
