//! Distinct operator - stateful, collapses multiplicities to 0 or 1.

use std::{collections::HashMap, hash::Hash};

use crate::change::Diff;

use super::relation::{Op, Relation};

/// A distinct operator - outputs each tuple at most once.
/// Tracks input multiplicities to emit +1 when count goes from 0 to positive,
/// and -1 when count goes from positive to 0.
pub struct DistinctOp<T, R>
where
    R: Op<T>,
{
    pub(super) inner: R,
    /// Track input multiplicities
    counts: HashMap<T, i64>,
}

impl<T: Clone + Eq + Hash, R> Op<T> for DistinctOp<T, R>
where
    R: Op<T>,
{
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        let counts = &mut self.counts;
        self.inner.foreach(&mut |t, diff| {
            let old_count = *counts.get(&t).unwrap_or(&0);
            let new_count = old_count + diff.0;

            if new_count == 0 {
                counts.remove(&t);
            } else {
                counts.insert(t.clone(), new_count);
            }

            let was_present = old_count > 0;
            let is_present = new_count > 0;

            if !was_present && is_present {
                consumer(t, Diff(1)); // appeared
            } else if was_present && !is_present {
                consumer(t, Diff(-1)); // disappeared
            }
            // else: no change to output
        });
    }
}

/// Create a distinct relation.
pub fn distinct<T, R>(input: Relation<R>) -> Relation<DistinctOp<T, R>>
where
    R: Op<T>,
{
    Relation::new(DistinctOp {
        inner: input.inner,
        counts: HashMap::new(),
    })
}
