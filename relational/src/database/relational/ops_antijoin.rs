//! Antijoin operator - filters left to tuples whose key is NOT in right.

use std::collections::HashMap;
use std::hash::Hash;

use contiguous_data::{Diff, Multiset};

use super::relation::{Op, Relation, assert_same_commit_id};

/// Antijoin operator - keeps (K, V) tuples from left where K is NOT in right.
/// Tracks both inputs to compute correct output deltas.
pub struct AntijoinOp<K, V, RL, RR>
where
    K: Eq + Hash + Clone,
    RL: Op<(K, V)>,
    RR: Op<K>,
{
    left: Relation<RL>,
    right: Relation<RR>,
    /// Index of left tuples by key: key -> multiset of values
    left_index: HashMap<K, Multiset<V>>,
    /// Count of right keys (positive = key is present, blocks output)
    right_counts: Multiset<K>,
}

impl<K, V, RL, RR> Op<(K, V)> for AntijoinOp<K, V, RL, RR>
where
    K: Clone + Eq + Hash,
    V: Clone + Eq + Hash,
    RL: Op<(K, V)>,
    RR: Op<K>,
{
    fn foreach(&mut self, mut consumer: impl FnMut((K, V), Diff)) {
        // Process left changes first
        self.left.foreach(|(k, v), l_diff| {
            let right_count = self.right_counts.get(&k);
            // Only emit if key is not blocked by right side (blocked when count != 0)
            if right_count == 0 && l_diff != 0 {
                consumer((k.clone(), v.clone()), l_diff);
            }
            // Update left index
            let entry = self.left_index.entry(k.clone()).or_default();
            entry.update(v, l_diff);
            if entry.is_empty() {
                self.left_index.remove(&k);
            }
        });

        // Process right changes - these can block/unblock left tuples
        self.right.foreach(|k, r_diff| {
            let old_count = self.right_counts.get(&k);
            let new_count = old_count + r_diff;

            let was_blocked = old_count != 0;
            let is_blocked = new_count != 0;

            if was_blocked != is_blocked {
                // Blocking state changed - emit/retract all left tuples with this key
                if let Some(lefts) = self.left_index.get(&k) {
                    for (v, l_count) in lefts.iter_with_multiplicity() {
                        if l_count != 0 {
                            // If now blocked, retract; if now unblocked, emit
                            let output_diff = if is_blocked { -l_count } else { l_count };
                            consumer((k.clone(), v.clone()), output_diff);
                        }
                    }
                }
            }

            // Update right count
            self.right_counts.update(k, r_diff);
        });
    }
}

impl<RL> Relation<RL> {
    /// Antijoin - filter to tuples whose key is NOT in right (without consolidation).
    /// Left input is (K, V), right input is K. Output is (K, V).
    pub fn antijoin_<K, V, RR>(self, right: Relation<RR>) -> Relation<impl Op<(K, V)>>
    where
        RL: Op<(K, V)>,
        K: Eq + Hash + Clone,
        V: Eq + Hash + Clone,
        RR: Op<K>,
    {
        assert_same_commit_id(&self.commit_id, &right.commit_id);
        let left_node = self.node_id;
        let right_node = right.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        Relation::new(
            AntijoinOp {
                left: self,
                right,
                left_index: HashMap::new(),
                right_counts: Multiset::new(),
            },
            commit_id,
            graph,
            "antijoin",
            vec![left_node, right_node],
        )
    }

    /// Antijoin - filter to tuples whose key is NOT in right.
    /// Left input is (K, V), right input is K. Output is (K, V).
    pub fn antijoin<K, V, RR>(self, right: Relation<RR>) -> Relation<impl Op<(K, V)>>
    where
        RL: Op<(K, V)>,
        K: Eq + Hash + Clone,
        V: Eq + Hash + Clone,
        RR: Op<K>,
    {
        let result = self.antijoin_(right);
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_passthrough_();
        result
    }
}
