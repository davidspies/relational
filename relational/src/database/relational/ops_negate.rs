//! Negate operator.

use std::hash::Hash;

use crate::change::Diff;

use super::relation::{Op, Relation};

/// A negate operator - negates all diffs.
pub struct NegateOp<T, R>
where
    R: Op<T>,
{
    inner: Relation<R>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, R> Op<T> for NegateOp<T, R>
where
    R: Op<T>,
{
    fn foreach(&mut self, mut consumer: impl FnMut(T, Diff)) {
        self.inner.foreach(|t, diff| {
            consumer(t, -diff);
        });
    }
}

impl<R> Relation<R> {
    /// Negate all diffs (without consolidation).
    pub fn negate_<T>(self) -> Relation<impl Op<T>>
    where
        R: Op<T>,
        T: Eq + Hash,
    {
        let node_id = self.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        Relation::new(
            NegateOp {
                inner: self,
                _phantom: std::marker::PhantomData,
            },
            commit_id,
            graph,
            "negate",
            vec![node_id],
        )
    }

    /// Negate all diffs.
    pub fn negate<T>(self) -> Relation<impl Op<T>>
    where
        R: Op<T>,
        T: Eq + Hash,
    {
        let result = self.negate_();
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_();
        result
    }
}
