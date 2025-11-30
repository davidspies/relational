//! Union operator - stateless, combines two relations.

use crate::change::Diff;

use super::relation::Op;

/// A union relation - combines changes from both inputs.
pub struct UnionRelation<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    left: L,
    right: R,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, L, R> Op<T> for UnionRelation<T, L, R>
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
pub fn union<T, L, R>(left: L, right: R) -> UnionRelation<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    UnionRelation {
        left,
        right,
        _phantom: std::marker::PhantomData,
    }
}
