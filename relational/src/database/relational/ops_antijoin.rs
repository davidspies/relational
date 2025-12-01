//! Antijoin operator - filters left to tuples whose key is NOT in right.

use std::collections::HashMap;
use std::hash::Hash;

use crate::change::Diff;
use crate::collection::Multiset;

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
    right_counts: HashMap<K, i64>,
}

impl<K, V, RL, RR> Op<(K, V)> for AntijoinOp<K, V, RL, RR>
where
    K: Clone + Eq + Hash,
    V: Clone + Eq + Hash,
    RL: Op<(K, V)>,
    RR: Op<K>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut((K, V), Diff)) {
        // Collect changes from both sides
        let mut left_changes = Multiset::new();
        let mut right_changes = Multiset::new();

        self.left.foreach(&mut |kv, diff| {
            left_changes.update(kv, diff);
        });
        self.right.foreach(&mut |k, diff| {
            right_changes.update(k, diff);
        });

        // Process left changes first
        for ((k, v), l_diff) in left_changes {
            let right_count = *self.right_counts.get(&k).unwrap_or(&0);
            // Only emit if key is not blocked by right side
            if right_count <= 0 && l_diff.0 != 0 {
                consumer((k.clone(), v.clone()), l_diff);
            }
            // Update left index
            let entry = self.left_index.entry(k).or_default();
            entry.update(v, l_diff);
        }

        // Process right changes - these can block/unblock left tuples
        for (k, r_diff) in right_changes {
            let old_count = *self.right_counts.get(&k).unwrap_or(&0);
            let new_count = old_count + r_diff.0;

            let was_blocked = old_count > 0;
            let is_blocked = new_count > 0;

            if was_blocked != is_blocked {
                // Blocking state changed - emit/retract all left tuples with this key
                if let Some(lefts) = self.left_index.get(&k) {
                    for (v, Diff(l_count)) in lefts.iter_with_multiplicity() {
                        if l_count != 0 {
                            // If now blocked, retract; if now unblocked, emit
                            let output_diff = if is_blocked { -l_count } else { l_count };
                            consumer((k.clone(), v.clone()), Diff(output_diff));
                        }
                    }
                }
            }

            // Update right count
            if new_count == 0 {
                self.right_counts.remove(&k);
            } else {
                self.right_counts.insert(k, new_count);
            }
        }
    }
}

impl<RL> Relation<RL> {
    /// Antijoin - filter to tuples whose key is NOT in right.
    /// Left input is (K, V), right input is K. Output is (K, V).
    pub fn antijoin<K, V, RR>(self, right: Relation<RR>) -> Relation<AntijoinOp<K, V, RL, RR>>
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
                right_counts: HashMap::new(),
            },
            commit_id,
            graph,
            "antijoin",
            vec![left_node, right_node],
        )
    }
}
