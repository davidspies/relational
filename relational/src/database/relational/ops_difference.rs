//! Difference operator - set difference (left - right).
//! Implemented as distinct(left + negate(right)).

use std::hash::Hash;

use crate::change::Diff;

use super::ops_distinct::{DistinctOp, distinct};
use super::relation::{Op, Relation, assert_same_commit_id};

/// A negate operator - negates all diffs.
pub struct NegateOp<T, R>
where
    R: Op<T>,
{
    inner: R,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, R> Op<T> for NegateOp<T, R>
where
    R: Op<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        self.inner.foreach(&mut |t, diff| {
            consumer(t, Diff(-diff.0));
        });
    }
}

/// Create a negate relation.
pub fn negate<T, R>(input: Relation<R>) -> Relation<NegateOp<T, R>>
where
    R: Op<T>,
{
    Relation {
        inner: NegateOp {
            inner: input.inner,
            _phantom: std::marker::PhantomData,
        },
        commit_id: input.commit_id,
    }
}

/// A difference operator - combines left with negated right, then distinct.
pub struct DifferenceOp<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    inner: DistinctOp<T, DifferenceUnion<T, L, R>>,
}

/// Union of left and negated right for difference.
struct DifferenceUnion<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    left: L,
    right: NegateOp<T, R>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, L, R> Op<T> for DifferenceUnion<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        self.left.foreach(consumer);
        self.right.foreach(consumer);
    }
}

impl<T: Clone + Eq + Hash, L, R> Op<T> for DifferenceOp<T, L, R>
where
    L: Op<T>,
    R: Op<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        self.inner.foreach(consumer);
    }
}

/// Create a difference relation (left - right).
pub fn difference<T, L, R>(left: Relation<L>, right: Relation<R>) -> Relation<DifferenceOp<T, L, R>>
where
    L: Op<T>,
    R: Op<T>,
{
    assert_same_commit_id(&left.commit_id, &right.commit_id);
    let commit_id = left.commit_id.clone();
    let union = DifferenceUnion {
        left: left.inner,
        right: NegateOp {
            inner: right.inner,
            _phantom: std::marker::PhantomData,
        },
        _phantom: std::marker::PhantomData,
    };
    Relation {
        inner: DifferenceOp {
            inner: distinct(Relation::new(union, commit_id.clone())).inner,
        },
        commit_id,
    }
}
