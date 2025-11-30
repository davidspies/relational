//! Output - accumulates relation changes into a queryable Multiset.

use crate::Tuple;
use crate::change::Change;
use crate::collection::Multiset;

use super::relation::Relation;

/// An output that accumulates changes from a relation into a Multiset.
///
/// Call `update()` to pull pending changes, then query the accumulated state.
/// Changes accumulate - you don't need to call `update()` every iteration.
///
/// Default type parameter allows `Output<T>` as shorthand for boxed relations.
pub struct Output<T: Tuple, R: Relation<T> = Box<dyn Relation<T>>> {
    relation: R,
    state: Multiset<T>,
}

impl<T: Tuple + 'static, R: Relation<T>> Output<T, R> {
    /// Create a new output wrapping the given relation.
    pub(crate) fn new(relation: R) -> Self {
        Output {
            relation,
            state: Multiset::new(),
        }
    }

    /// Pull all pending changes from the relation into the accumulated state.
    pub(crate) fn update(&mut self) {
        self.relation.foreach(&mut |t, diff| {
            self.state.apply_change(Change::new(t, diff));
        });
    }

    /// Collect all tuples with positive multiplicity into a Vec.
    /// Automatically calls `update()` first to pull pending changes.
    pub fn collect(&mut self) -> Vec<T> {
        self.update();
        self.state.iter().cloned().collect()
    }
}

/// Create an output from a relation.
pub fn output<T: Tuple + 'static, R: Relation<T>>(relation: R) -> Output<T, R> {
    Output::new(relation)
}
