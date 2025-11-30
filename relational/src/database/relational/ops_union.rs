//! Union operator - stateless, combines two relations.

use crate::change::Diff;

use super::relation::{Op, Relation};

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

/// Create a union relation.
pub fn union<T, L, R>(left: Relation<L>, right: Relation<R>) -> Relation<UnionOp<T, L, R>>
where
    L: Op<T>,
    R: Op<T>,
{
    Relation::new(UnionOp {
        left: left.inner,
        right: right.inner,
        _phantom: std::marker::PhantomData,
    })
}
