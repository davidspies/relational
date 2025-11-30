//! Join operator - stateful, joins two relations on a key.

use std::collections::HashMap;
use std::hash::Hash;

use crate::change::Diff;
use crate::collection::Multiset;

use super::relation::{Op, Relation, assert_same_commit_id};

/// A join operator - joins left and right on matching keys.
/// Tracks both input states to compute correct output deltas.
pub struct JoinOp<L, R, K, FL, FR, RL, RR>
where
    K: Eq + Hash + Clone,
    FL: Fn(&L) -> K,
    FR: Fn(&R) -> K,
    RL: Op<L>,
    RR: Op<R>,
{
    left: RL,
    right: RR,
    key_left: FL,
    key_right: FR,
    /// Index of left tuples by key: key -> [(tuple, count)]
    left_index: HashMap<K, Multiset<L>>,
    /// Index of right tuples by key: key -> [(tuple, count)]
    right_index: HashMap<K, Multiset<R>>,
}

impl<L, R, K, FL, FR, RL, RR> Op<(L, R)> for JoinOp<L, R, K, FL, FR, RL, RR>
where
    L: Clone + Eq + Hash,
    R: Clone + Eq + Hash,
    K: Eq + Hash + Clone,
    FL: Fn(&L) -> K,
    FR: Fn(&R) -> K,
    RL: Op<L>,
    RR: Op<R>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut((L, R), Diff)) {
        // Collect changes from both sides using Multiset to consolidate duplicates
        let mut left_changes = Multiset::new();
        let mut right_changes = Multiset::new();

        self.left.foreach(&mut |l, diff| {
            left_changes.update(l, diff);
        });
        self.right.foreach(&mut |r, diff| {
            right_changes.update(r, diff);
        });

        // Process left changes - join with existing right state
        for (l, l_diff) in left_changes {
            let k = (self.key_left)(&l);

            // Join with existing right tuples
            if let Some(rights) = self.right_index.get(&k) {
                for (r, Diff(r_count)) in rights.iter_with_multiplicity() {
                    let output_diff = Diff(l_diff.0 * r_count);
                    if output_diff.0 != 0 {
                        consumer((l.clone(), r.clone()), output_diff);
                    }
                }
            }

            // Update left index
            let entry = self.left_index.entry(k).or_default();
            entry.update(l, l_diff);
        }

        // Process right changes - join with updated left state (includes new left tuples)
        for (r, r_diff) in right_changes {
            let k = (self.key_right)(&r);

            // Join with left tuples (now includes newly added ones)
            if let Some(lefts) = self.left_index.get(&k) {
                for (l, Diff(l_count)) in lefts.iter_with_multiplicity() {
                    let output_diff = Diff(l_count * r_diff.0);
                    if output_diff.0 != 0 {
                        consumer((l.clone(), r.clone()), output_diff);
                    }
                }
            }

            // Update right index
            let entry = self.right_index.entry(k).or_default();
            entry.update(r, r_diff);
        }
    }
}

impl<RL> Relation<RL> {
    /// Join two relations on matching keys.
    pub fn join<L, R, K, FL, FR, RR>(
        self,
        right: Relation<RR>,
        key_left: FL,
        key_right: FR,
    ) -> Relation<JoinOp<L, R, K, FL, FR, RL, RR>>
    where
        RL: Op<L>,
        K: Eq + Hash + Clone,
        FL: Fn(&L) -> K,
        FR: Fn(&R) -> K,
        RR: Op<R>,
    {
        assert_same_commit_id(&self.commit_id, &right.commit_id);
        let left_node = self.node_id;
        let right_node = right.node_id;
        Relation::new(
            JoinOp {
                left: self.inner,
                right: right.inner,
                key_left,
                key_right,
                left_index: HashMap::new(),
                right_index: HashMap::new(),
            },
            self.commit_id,
            self.graph,
            "join",
            vec![left_node, right_node],
        )
    }
}
