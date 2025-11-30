//! FlatMap operator - stateless, transforms each tuple into zero or more tuples.

use crate::change::Diff;

use super::relation::Op;

/// A flat_map relation - transforms each tuple into zero or more tuples.
pub struct FlatMapRelation<T, U, I, F, R>
where
    I: IntoIterator<Item = U>,
    F: Fn(T) -> I,
    R: Op<T>,
{
    inner: R,
    f: F,
    _phantom: std::marker::PhantomData<(T, I)>,
}

impl<T, U, I, F, R> Op<U> for FlatMapRelation<T, U, I, F, R>
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

/// Create a flat_map relation.
pub fn flat_map<T, U, I, F, R>(input: R, f: F) -> FlatMapRelation<T, U, I, F, R>
where
    I: IntoIterator<Item = U>,
    F: Fn(T) -> I,
    R: Op<T>,
{
    FlatMapRelation {
        inner: input,
        f,
        _phantom: std::marker::PhantomData,
    }
}
