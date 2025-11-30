//! Join operator - stateful, joins two relations on a key.

use std::collections::HashMap;
use std::hash::Hash;

use crate::Tuple;
use crate::change::Diff;
use crate::collection::Multiset;

use super::relation::Relation;

/// A join relation - joins left and right on matching keys.
/// Tracks both input states to compute correct output deltas.
pub struct JoinRelation<L, R, K, FL, FR, RL, RR>
where
    L: Tuple,
    R: Tuple,
    K: Eq + Hash + Clone,
    FL: Fn(&L) -> K,
    FR: Fn(&R) -> K,
    RL: Relation<L>,
    RR: Relation<R>,
{
    left: RL,
    right: RR,
    key_left: FL,
    key_right: FR,
    /// Index of left tuples by key: key -> [(tuple, count)]
    left_index: HashMap<K, Vec<(L, i64)>>,
    /// Index of right tuples by key: key -> [(tuple, count)]
    right_index: HashMap<K, Vec<(R, i64)>>,
}

impl<L, R, K, FL, FR, RL, RR> Relation<(L, R)> for JoinRelation<L, R, K, FL, FR, RL, RR>
where
    L: Tuple + 'static,
    R: Tuple + 'static,
    K: Eq + Hash + Clone + 'static,
    FL: Fn(&L) -> K + 'static,
    FR: Fn(&R) -> K + 'static,
    RL: Relation<L>,
    RR: Relation<R>,
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
                for (r, r_count) in rights {
                    let output_diff = Diff(l_diff.0 * r_count);
                    if output_diff.0 != 0 {
                        consumer((l.clone(), r.clone()), output_diff);
                    }
                }
            }

            // Update left index
            let entry = self.left_index.entry(k).or_default();
            update_index_entry(entry, l, l_diff.0);
        }

        // Process right changes - join with updated left state (includes new left tuples)
        for (r, r_diff) in right_changes {
            let k = (self.key_right)(&r);

            // Join with left tuples (now includes newly added ones)
            if let Some(lefts) = self.left_index.get(&k) {
                for (l, l_count) in lefts {
                    let output_diff = Diff(l_count * r_diff.0);
                    if output_diff.0 != 0 {
                        consumer((l.clone(), r.clone()), output_diff);
                    }
                }
            }

            // Update right index
            let entry = self.right_index.entry(k).or_default();
            update_index_entry(entry, r, r_diff.0);
        }
    }
}

/// Update an index entry, maintaining the invariant that entries with count 0 are removed.
fn update_index_entry<T: Tuple>(entry: &mut Vec<(T, i64)>, tuple: T, diff: i64) {
    if let Some(pos) = entry.iter().position(|(t, _)| t == &tuple) {
        entry[pos].1 += diff;
        if entry[pos].1 == 0 {
            entry.swap_remove(pos);
        }
    } else if diff != 0 {
        entry.push((tuple, diff));
    }
}

/// Create a join relation.
pub fn join<L, R, K, FL, FR, RL, RR>(
    left: RL,
    right: RR,
    key_left: FL,
    key_right: FR,
) -> JoinRelation<L, R, K, FL, FR, RL, RR>
where
    L: Tuple + 'static,
    R: Tuple + 'static,
    K: Eq + Hash + Clone + 'static,
    FL: Fn(&L) -> K + 'static,
    FR: Fn(&R) -> K + 'static,
    RL: Relation<L>,
    RR: Relation<R>,
{
    JoinRelation {
        left,
        right,
        key_left,
        key_right,
        left_index: HashMap::new(),
        right_index: HashMap::new(),
    }
}
