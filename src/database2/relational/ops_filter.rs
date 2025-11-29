//! Filter operator - stateless, filters tuples by predicate.

use crate::change::Diff;
use crate::Tuple;

use super::relation::Relation;

/// A filter relation - keeps tuples matching the predicate.
pub struct FilterRelation<T, F, R>
where
    T: Tuple,
    F: Fn(&T) -> bool,
    R: Relation<T>,
{
    inner: R,
    pred: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, F, R> Relation<T> for FilterRelation<T, F, R>
where
    T: Tuple + 'static,
    F: Fn(&T) -> bool + 'static,
    R: Relation<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(&T, Diff)) {
        let pred = &self.pred;
        self.inner.foreach(&mut |t, diff| {
            if pred(t) {
                consumer(t, diff);
            }
        });
    }
}

/// Create a filter relation.
pub fn filter<T, F, R>(input: R, pred: F) -> FilterRelation<T, F, R>
where
    T: Tuple + 'static,
    F: Fn(&T) -> bool + 'static,
    R: Relation<T>,
{
    FilterRelation {
        inner: input,
        pred,
        _phantom: std::marker::PhantomData,
    }
}
