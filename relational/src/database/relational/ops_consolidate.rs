//! Consolidate operator - collects changes and forwards without duplicates or zeros.

use std::hash::Hash;

use crate::Diff;
use crate::collection::Multiset;

use super::relation::{Op, Relation};

/// Consolidate operator that merges duplicate changes.
pub struct ConsolidateOp<T, R: Op<T>> {
    upstream: Relation<R>,
    pending: Multiset<T>,
}

impl<T: Clone + Eq + Hash, R: Op<T>> Op<T> for ConsolidateOp<T, R> {
    fn foreach(&mut self, mut consumer: impl FnMut(T, Diff)) {
        // Pull all changes from upstream into the pending multiset
        self.upstream.dump_to_multiset(&mut self.pending);

        // Drain the multiset, forwarding non-zero entries
        for (t, diff) in self.pending.drain() {
            consumer(t, diff);
        }
    }
}

impl<R> Relation<R> {
    /// Consolidate changes by merging duplicates.
    ///
    /// This operator collects all pending changes and merges entries with the
    /// same key, forwarding only non-zero multiplicities. This is useful when
    /// you have a relation that may produce duplicate changes that should be
    /// combined before further processing.
    ///
    /// # Example
    /// If upstream produces: `(a, +1), (a, +1), (a, -1)`
    /// Consolidate outputs: `(a, +1)` (the net change)
    pub fn consolidate<T>(self) -> Relation<ConsolidateOp<T, R>>
    where
        T: Clone + Eq + Hash,
        R: Op<T>,
    {
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        let parent = self.node_id;
        Relation::new(
            ConsolidateOp {
                upstream: self,
                pending: Multiset::new(),
            },
            commit_id,
            graph,
            "consolidate",
            vec![parent],
        )
    }
}
