//! Difference operator - set difference (left - right).
//! Implemented as distinct(left + negate(right)).

use crate::Tuple;
use crate::change::Diff;

use super::ops_distinct::{DistinctRelation, distinct};
use super::relation::Relation;

/// A negate relation - negates all diffs.
pub struct NegateRelation<T, R>
where
    T: Tuple,
    R: Relation<T>,
{
    inner: R,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, R> Relation<T> for NegateRelation<T, R>
where
    T: Tuple + 'static,
    R: Relation<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        self.inner.foreach(&mut |t, diff| {
            consumer(t, Diff(-diff.0));
        });
    }
}

/// Create a negate relation.
pub fn negate<T, R>(input: R) -> NegateRelation<T, R>
where
    T: Tuple + 'static,
    R: Relation<T>,
{
    NegateRelation {
        inner: input,
        _phantom: std::marker::PhantomData,
    }
}

/// A difference relation - combines left with negated right, then distinct.
pub struct DifferenceRelation<T, L, R>
where
    T: Tuple,
    L: Relation<T>,
    R: Relation<T>,
{
    inner: DistinctRelation<T, DifferenceUnion<T, L, R>>,
}

/// Union of left and negated right for difference.
pub struct DifferenceUnion<T, L, R>
where
    T: Tuple,
    L: Relation<T>,
    R: Relation<T>,
{
    left: L,
    right: NegateRelation<T, R>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, L, R> Relation<T> for DifferenceUnion<T, L, R>
where
    T: Tuple + 'static,
    L: Relation<T>,
    R: Relation<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        self.left.foreach(consumer);
        self.right.foreach(consumer);
    }
}

impl<T, L, R> Relation<T> for DifferenceRelation<T, L, R>
where
    T: Tuple + 'static,
    L: Relation<T>,
    R: Relation<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        self.inner.foreach(consumer);
    }
}

/// Create a difference relation (left - right).
pub fn difference<T, L, R>(left: L, right: R) -> DifferenceRelation<T, L, R>
where
    T: Tuple + 'static,
    L: Relation<T>,
    R: Relation<T>,
{
    let union = DifferenceUnion {
        left,
        right: negate(right),
        _phantom: std::marker::PhantomData,
    };
    DifferenceRelation {
        inner: distinct(union),
    }
}
