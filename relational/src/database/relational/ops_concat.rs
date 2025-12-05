//! Union operator - stateless, combines two relations.

use std::hash::Hash;

use contiguous_data::Diff;

use super::op::Op;
use super::relation::{Relation, assert_same_commit_id};

/// A concat operator - combines changes from both inputs.
pub struct ConcatOp<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    left: Relation<L>,
    right: Relation<R>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, L, R> Op<T> for ConcatOp<T, L, R>
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
    /// Combine two relations (without consolidation).
    pub fn concat_<T, R>(self, right: Relation<R>) -> Relation<impl Op<T>>
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
        Relation::new(
            ConcatOp {
                left: self,
                right,
                _phantom: std::marker::PhantomData,
            },
            commit_id,
            graph,
            "concat",
            vec![left_node, right_node],
        )
    }

    /// Combine two relations.
    pub fn concat<T, R>(self, right: Relation<R>) -> Relation<impl Op<T>>
    where
        L: Op<T>,
        R: Op<T>,
        T: Eq + Hash,
    {
        let result = self.concat_(right);
        #[cfg(feature = "consolidate_all")]
        let result = result.consolidate_passthrough_();
        result
    }
}
