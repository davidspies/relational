//! Map operator - stateless, transforms each tuple.

use crate::change::Diff;
use crate::Tuple;

use super::relation::Relation;

/// A map relation - transforms each tuple.
pub struct MapRelation<T, U, F, R>
where
    T: Tuple,
    U: Tuple,
    F: Fn(&T) -> U,
    R: Relation<T>,
{
    inner: R,
    f: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, U, F, R> Relation<U> for MapRelation<T, U, F, R>
where
    T: Tuple + 'static,
    U: Tuple + 'static,
    F: Fn(&T) -> U + 'static,
    R: Relation<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(&U, Diff)) {
        let f = &self.f;
        self.inner.foreach(&mut |t, diff| {
            consumer(&f(t), diff);
        });
    }
}

/// Create a map relation.
pub fn map<T, U, F, R>(input: R, f: F) -> MapRelation<T, U, F, R>
where
    T: Tuple + 'static,
    U: Tuple + 'static,
    F: Fn(&T) -> U + 'static,
    R: Relation<T>,
{
    MapRelation {
        inner: input,
        f,
        _phantom: std::marker::PhantomData,
    }
}
