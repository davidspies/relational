//! Negate operator.

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
    /// Negate all diffs.
    pub fn negate<T>(self) -> Relation<NegateOp<T, R>>
    where
        R: Op<T>,
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
}
