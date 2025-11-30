//! Union operator - stateless, combines two relations.

use crate::change::Diff;

use super::relation::{Op, Relation, assert_same_commit_id};

/// A union operator - combines changes from both inputs.
pub struct UnionOp<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    left: L,
    right: R,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, L, R> Op<T> for UnionOp<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        self.left.foreach(consumer);
        self.right.foreach(consumer);
    }
}

impl<L> Relation<L> {
    /// Combine two relations.
    pub fn union<T, R>(self, right: Relation<R>) -> Relation<UnionOp<T, L, R>>
    where
        L: Op<T>,
        R: Op<T>,
    {
        assert_same_commit_id(&self.commit_id, &right.commit_id);
        let left_node = self.node_id;
        let right_node = right.node_id;
        Relation::new(
            UnionOp {
                left: self.inner,
                right: right.inner,
                _phantom: std::marker::PhantomData,
            },
            self.commit_id,
            self.graph,
            "union",
            vec![left_node, right_node],
        )
    }
}
