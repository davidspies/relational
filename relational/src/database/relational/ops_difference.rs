//! Difference operator - set difference (left - right).
//! Implemented as distinct(left + negate(right)).

use std::hash::Hash;

use crate::change::Diff;

use super::ops_distinct::DistinctOp;
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

impl<R> Relation<R> {
    /// Negate all diffs.
    pub fn negate<T>(self) -> Relation<NegateOp<T, R>>
    where
        R: Op<T>,
    {
        Relation {
            inner: NegateOp {
                inner: self.inner,
                _phantom: std::marker::PhantomData,
            },
            commit_id: self.commit_id,
        }
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

impl<L> Relation<L> {
    /// Set difference (self - right).
    pub fn difference<T, R>(self, right: Relation<R>) -> Relation<DifferenceOp<T, L, R>>
    where
        L: Op<T>,
        R: Op<T>,
    {
        assert_same_commit_id(&self.commit_id, &right.commit_id);
        let commit_id = self.commit_id.clone();
        let union = DifferenceUnion {
            left: self.inner,
            right: NegateOp {
                inner: right.inner,
                _phantom: std::marker::PhantomData,
            },
            _phantom: std::marker::PhantomData,
        };
        Relation {
            inner: DifferenceOp {
                inner: Relation::new(union, commit_id.clone()).distinct().inner,
            },
            commit_id,
        }
    }
}
