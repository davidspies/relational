//! Map operator - stateless, transforms each tuple into exactly one tuple.

use std::hash::Hash;

use crate::change::Diff;

use super::relation::{Op, Relation};

/// A map operator - transforms each tuple into exactly one tuple.
pub struct MapOp<T, U, F, R>
where
    F: Fn(T) -> U,
    R: Op<T>,
{
    inner: Relation<R>,
    f: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, U, F, R> Op<U> for MapOp<T, U, F, R>
where
    F: Fn(T) -> U,
    R: Op<T>,
{
    fn foreach(&mut self, mut consumer: impl FnMut(U, Diff)) {
        let f = &self.f;
        self.inner.foreach(|t, diff| {
            consumer(f(t), diff);
        });
    }
}

impl<R> Relation<R> {
    /// Transform each tuple (without consolidation).
    pub fn map_<T, U, F>(self, f: F) -> Relation<impl Op<U>>
    where
        R: Op<T>,
        F: Fn(T) -> U,
        U: Eq + Hash,
    {
        let node_id = self.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        Relation::new(
            MapOp {
                inner: self,
                f,
                _phantom: std::marker::PhantomData,
            },
            commit_id,
            graph,
            "map",
            vec![node_id],
        )
    }

    /// Transform each tuple.
    pub fn map<T, U, F>(self, f: F) -> Relation<impl Op<U>>
    where
        R: Op<T>,
        F: Fn(T) -> U,
        U: Eq + Hash,
    {
        let result = self.map_(f);
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_();
        result
    }
}
