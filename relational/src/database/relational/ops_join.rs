//! Left join operator - joins two relations, preserving all left tuples.

use std::hash::Hash;

use contiguous_data::{Diff, L2Multiset, Multiset};

use super::op::Op;
use super::relation::{Relation, assert_same_commit_id};

/// Left join operator - joins left and right on matching keys.
/// Input is (K, V) tuples on both sides, output is (K, (V1, Option<V2>)) tuples.
/// All left tuples are preserved; those without matching right keys get None.
pub struct LeftJoinOp<K, V1, V2, RL, RR>
where
    K: Eq + Hash + Clone,
    RL: Op<(K, V1)>,
    RR: Op<(K, V2)>,
{
    left: Relation<RL>,
    right: Relation<RR>,
    left_index: L2Multiset<K, V1>,
    right_index: L2Multiset<K, V2>,
    /// Total count of each key on right side (0 means no matches)
    right_counts: Multiset<K>,
}

impl<K, V1, V2, RL, RR> Op<(K, (V1, Option<V2>))> for LeftJoinOp<K, V1, V2, RL, RR>
where
    K: Clone + Eq + Hash,
    V1: Clone + Eq + Hash,
    V2: Clone + Eq + Hash,
    RL: Op<(K, V1)>,
    RR: Op<(K, V2)>,
{
    fn foreach(&mut self, mut consumer: impl FnMut((K, (V1, Option<V2>)), Diff)) {
        // Process left changes first
        self.left.foreach(|(k, v1), l_diff| {
            let right_count = self.right_counts.get(&k);

            if right_count == 0 {
                // No matching right tuples - emit with None
                if l_diff != 0 {
                    consumer((k.clone(), (v1.clone(), None)), l_diff);
                }
            } else {
                // Has matching right tuples - emit with each one
                for (v2, r_count) in self.right_index.iter_with_multiplicity(&k) {
                    let output_diff = l_diff * r_count;
                    if output_diff != 0 {
                        consumer((k.clone(), (v1.clone(), Some(v2.clone()))), output_diff);
                    }
                }
            }

            self.left_index.update(k, v1, l_diff);
        });

        // Process right changes
        self.right.foreach(|(k, v2), r_diff| {
            let old_count = self.right_counts.get(&k);
            let new_count = old_count + r_diff;

            let had_matches = old_count != 0;
            let has_matches = new_count != 0;

            for (v1, l_count) in self.left_index.iter_with_multiplicity(&k) {
                if !had_matches && has_matches {
                    // Transitioning from no matches to having matches - retract None
                    consumer((k.clone(), (v1.clone(), None)), -l_count);
                }

                if had_matches && !has_matches {
                    // Transitioning from having matches to no matches - emit None
                    consumer((k.clone(), (v1.clone(), None)), l_count);
                }

                // Emit/retract the Some for this specific v2 change
                let output_diff = l_count * r_diff;
                if output_diff != 0 {
                    consumer((k.clone(), (v1.clone(), Some(v2.clone()))), output_diff);
                }
            }

            self.right_counts.update(k.clone(), r_diff);
            self.right_index.update(k, v2, r_diff);
        });
    }
}

impl<RL> Relation<RL> {
    /// Left join two relations on matching keys (without consolidation).
    /// Both inputs must be (K, V) tuples. Output is (K, (V1, Option<V2>)) tuples.
    /// All left tuples are preserved; those without matching right keys get None.
    pub fn left_join_<K, V1, V2, RR>(
        self,
        right: Relation<RR>,
    ) -> Relation<impl Op<(K, (V1, Option<V2>))>>
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
            LeftJoinOp {
                left: self,
                right,
                left_index: L2Multiset::new(),
                right_index: L2Multiset::new(),
                right_counts: Multiset::new(),
            },
            commit_id,
            graph,
            "left_join",
            vec![left_node, right_node],
        )
    }

    /// Left join two relations on matching keys.
    /// Both inputs must be (K, V) tuples. Output is (K, (V1, Option<V2>)) tuples.
    /// All left tuples are preserved; those without matching right keys get None.
    pub fn left_join<K, V1, V2, RR>(
        self,
        right: Relation<RR>,
    ) -> Relation<impl Op<(K, (V1, Option<V2>))>>
    where
        RL: Op<(K, V1)>,
        K: Clone + Eq + Hash,
        V1: Clone + Eq + Hash,
        V2: Clone + Eq + Hash,
        RR: Op<(K, V2)>,
    {
        let result = self.left_join_(right);
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_passthrough_();
        result
    }
}
