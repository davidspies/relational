//! FlatMap operator - stateless, transforms each tuple into zero or more tuples.

use std::hash::Hash;

use contiguous_data::Diff;

use super::op::Op;
use super::relation::Relation;

/// A flat_map operator - transforms each tuple into zero or more tuples.
pub struct FlatMapOp<T, U, I, F, R>
where
    I: IntoIterator<Item = U>,
    F: Fn(T) -> I,
    R: Op<T>,
{
    inner: Relation<R>,
    f: F,
    _phantom: std::marker::PhantomData<(T, I)>,
}

impl<T, U, I, F, R> Op<U> for FlatMapOp<T, U, I, F, R>
where
    I: IntoIterator<Item = U>,
    F: Fn(T) -> I,
    R: Op<T>,
{
    fn foreach(&mut self, mut consumer: impl FnMut(U, Diff)) {
        let f = &self.f;
        self.inner.foreach(|t, diff| {
            for u in f(t) {
                consumer(u, diff);
            }
        });
    }
}

impl<R> Relation<R> {
    /// Transform each tuple into zero or more tuples (without consolidation).
    pub fn flat_map_<T, U, I, F>(self, f: F) -> Relation<impl Op<U>>
    where
        R: Op<T>,
        I: IntoIterator<Item = U>,
        F: Fn(T) -> I,
        U: Eq + Hash,
    {
        let node_id = self.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        Relation::new(
            FlatMapOp {
                inner: self,
                f,
                _phantom: std::marker::PhantomData,
            },
            commit_id,
            graph,
            "flat_map",
            vec![node_id],
        )
    }

    /// Transform each tuple into zero or more tuples.
    pub fn flat_map<T, U, I, F>(self, f: F) -> Relation<impl Op<U>>
    where
        R: Op<T>,
        I: IntoIterator<Item = U>,
        F: Fn(T) -> I,
        U: Eq + Hash,
    {
        let result = self.flat_map_(f);
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_passthrough_();
        result
    }
}
