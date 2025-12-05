//! Join operator - stateful, joins two relations on a key.

use std::hash::Hash;

use contiguous_data::{Diff, L2Multiset};

use super::op::Op;
use super::relation::{Relation, assert_same_commit_id};

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
    /// Index of left tuples by key
    left_index: L2Multiset<K, V1>,
    /// Index of right tuples by key
    right_index: L2Multiset<K, V2>,
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
        // Process left changes - join with existing right state
        self.left.foreach(|(k, v1), l_diff| {
            // Join with existing right tuples
            for (v2, r_count) in self.right_index.iter_with_multiplicity(&k) {
                let output_diff = l_diff * r_count;
                if output_diff != 0 {
                    consumer((k.clone(), (v1.clone(), v2.clone())), output_diff);
                }
            }

            // Update left index
            self.left_index.update(k, v1, l_diff);
        });

        // Process right changes - join with updated left state (includes new left tuples)
        self.right.foreach(|(k, v2), r_diff| {
            // Join with left tuples (now includes newly added ones)
            for (v1, l_count) in self.left_index.iter_with_multiplicity(&k) {
                let output_diff = l_count * r_diff;
                if output_diff != 0 {
                    consumer((k.clone(), (v1.clone(), v2.clone())), output_diff);
                }
            }

            // Update right index
            self.right_index.update(k, v2, r_diff);
        });
    }
}

impl<RL> Relation<RL> {
    /// Join two relations on matching keys (without consolidation).
    /// Both inputs must be (K, V) tuples. Output is (K, (V1, V2)) tuples.
    pub fn join_<K, V1, V2, RR>(self, right: Relation<RR>) -> Relation<impl Op<(K, (V1, V2))>>
    where
        RL: Op<(K, V1)>,
        K: Clone + Eq + Hash,
        V1: Clone + Eq + Hash,
        V2: Clone + Eq + Hash,
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
                left_index: L2Multiset::new(),
                right_index: L2Multiset::new(),
            },
            commit_id,
            graph,
            "join",
            vec![left_node, right_node],
        )
    }

    /// Join two relations on matching keys.
    /// Both inputs must be (K, V) tuples. Output is (K, (V1, V2)) tuples.
    pub fn join<K, V1, V2, RR>(self, right: Relation<RR>) -> Relation<impl Op<(K, (V1, V2))>>
    where
        RL: Op<(K, V1)>,
        K: Clone + Eq + Hash,
        V1: Clone + Eq + Hash,
        V2: Clone + Eq + Hash,
        RR: Op<(K, V2)>,
    {
        let result = self.join_(right);
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_passthrough_();
        result
    }
}
