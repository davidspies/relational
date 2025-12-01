//! Union operator - stateless, combines two relations.

use std::hash::Hash;

use crate::change::Diff;

use super::relation::{Op, Relation, assert_same_commit_id};

/// A union operator - combines changes from both inputs.
pub struct UnionOp<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    left: Relation<L>,
    right: Relation<R>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, L, R> Op<T> for UnionOp<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    fn foreach(&mut self, mut consumer: impl FnMut(T, Diff)) {
        self.left.foreach(&mut consumer);
        self.right.foreach(consumer);
    }
}

impl<L> Relation<L> {
    /// Combine two relations.
    pub fn union<T, R>(self, right: Relation<R>) -> Relation<impl Op<T>>
    where
        L: Op<T>,
        R: Op<T>,
        T: Eq + Hash,
    {
        assert_same_commit_id(&self.commit_id, &right.commit_id);
        let left_node = self.node_id;
        let right_node = right.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();
        let result = Relation::new(
            UnionOp {
                left: self,
                right,
                _phantom: std::marker::PhantomData,
            },
            commit_id,
            graph,
            "union",
            vec![left_node, right_node],
        );
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_();
        result
    }
}
