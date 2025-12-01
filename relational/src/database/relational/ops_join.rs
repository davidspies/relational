//! Join operator - stateful, joins two relations on a key.

use std::collections::HashMap;
use std::hash::Hash;

use crate::change::Diff;
use crate::collection::Multiset;

use super::relation::{Op, Relation, assert_same_commit_id};

/// A join operator - joins left and right on matching keys.
/// Input is (K, V) tuples on both sides, output is (K, (V1, V2)) tuples.
/// Tracks both input states to compute correct output deltas.
pub struct JoinOp<K, V1, V2, RL, RR>
where
    K: Eq + Hash + Clone,
    RL: Op<(K, V1)>,
    RR: Op<(K, V2)>,
{
    left: Relation<RL>,
    right: Relation<RR>,
    /// Index of left tuples by key: key -> [(value, count)]
    left_index: HashMap<K, Multiset<V1>>,
    /// Index of right tuples by key: key -> [(value, count)]
    right_index: HashMap<K, Multiset<V2>>,
}

impl<K, V1, V2, RL, RR> Op<(K, (V1, V2))> for JoinOp<K, V1, V2, RL, RR>
where
    K: Clone + Eq + Hash,
    V1: Clone + Eq + Hash,
    V2: Clone + Eq + Hash,
    RL: Op<(K, V1)>,
    RR: Op<(K, V2)>,
{
    fn foreach(&mut self, mut consumer: impl FnMut((K, (V1, V2)), Diff)) {
        // Collect changes from both sides using Multiset to consolidate duplicates
        let mut left_changes = Multiset::new();
        let mut right_changes = Multiset::new();

        self.left.foreach(|kv, diff| {
            left_changes.update(kv, diff);
        });
        self.right.foreach(|kv, diff| {
            right_changes.update(kv, diff);
        });

        // Process left changes - join with existing right state
        for ((k, v1), l_diff) in left_changes {
            // Join with existing right tuples
            if let Some(rights) = self.right_index.get(&k) {
                for (v2, Diff(r_count)) in rights.iter_with_multiplicity() {
                    let output_diff = Diff(l_diff.0 * r_count);
                    if output_diff.0 != 0 {
                        consumer((k.clone(), (v1.clone(), v2.clone())), output_diff);
                    }
                }
            }

            // Update left index
            let entry = self.left_index.entry(k).or_default();
            entry.update(v1, l_diff);
        }

        // Process right changes - join with updated left state (includes new left tuples)
        for ((k, v2), r_diff) in right_changes {
            // Join with left tuples (now includes newly added ones)
            if let Some(lefts) = self.left_index.get(&k) {
                for (v1, Diff(l_count)) in lefts.iter_with_multiplicity() {
                    let output_diff = Diff(l_count * r_diff.0);
                    if output_diff.0 != 0 {
                        consumer((k.clone(), (v1.clone(), v2.clone())), output_diff);
                    }
                }
            }

            // Update right index
            let entry = self.right_index.entry(k).or_default();
            entry.update(v2, r_diff);
        }
    }
}

impl<RL> Relation<RL> {
    /// Join two relations on matching keys.
    /// Both inputs must be (K, V) tuples. Output is (K, (V1, V2)) tuples.
    pub fn join<K, V1, V2, RR>(self, right: Relation<RR>) -> Relation<JoinOp<K, V1, V2, RL, RR>>
    where
        RL: Op<(K, V1)>,
        K: Eq + Hash + Clone,
        RR: Op<(K, V2)>,
    {
        assert_same_commit_id(&self.commit_id, &right.commit_id);
        let left_node = self.node_id;
        let right_node = right.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        Relation::new(
            JoinOp {
                left: self,
                right,
                left_index: HashMap::new(),
                right_index: HashMap::new(),
            },
            commit_id,
            graph,
            "join",
            vec![left_node, right_node],
        )
    }
}
