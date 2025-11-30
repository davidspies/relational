//! FlatMap operator - stateless, transforms each tuple into zero or more tuples.

use crate::change::Diff;

use super::relation::{Op, Relation};

/// A flat_map operator - transforms each tuple into zero or more tuples.
pub struct FlatMapOp<T, U, I, F, R>
where
    I: IntoIterator<Item = U>,
    F: Fn(T) -> I,
    R: Op<T>,
{
    inner: R,
    f: F,
    _phantom: std::marker::PhantomData<(T, I)>,
}

impl<T, U, I, F, R> Op<U> for FlatMapOp<T, U, I, F, R>
where
    I: IntoIterator<Item = U>,
    F: Fn(T) -> I,
    R: Op<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(U, Diff)) {
        let f = &self.f;
        self.inner.foreach(&mut |t, diff| {
            for u in f(t) {
                consumer(u, diff);
            }
        });
    }
}

impl<R> Relation<R> {
    /// Transform each tuple into zero or more tuples.
    pub fn flat_map<T, U, I, F>(self, f: F) -> Relation<FlatMapOp<T, U, I, F, R>>
    where
        R: Op<T>,
        I: IntoIterator<Item = U>,
        F: Fn(T) -> I,
    {
        Relation {
            inner: FlatMapOp {
                inner: self.inner,
                f,
                _phantom: std::marker::PhantomData,
            },
            commit_id: self.commit_id,
        }
    }
}
