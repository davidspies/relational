//! Type-erased changes trait for checkpoint operations.

use crate::Tuple;
use crate::change::Change;
use crate::collection::Multiset;

/// Type-erased changes that can be manipulated.
pub trait AnyChanges: Send + Sync {
    /// Unapply these changes (negate and apply).
    fn unapply(&self, state: &mut dyn crate::dataflow::AnyCollection);
    /// Clone into a box.
    fn clone_box(&self) -> Box<dyn AnyChanges>;
    /// Downcast to mutable Any for type checking.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl<T: Tuple + Send + Sync> AnyChanges for Vec<Change<T>> {
    fn unapply(&self, state: &mut dyn crate::dataflow::AnyCollection) {
        if let Some(coll) = state.as_any_mut().downcast_mut::<Multiset<T>>() {
            let negated: Vec<Change<T>> = self
                .iter()
                .map(|c| Change {
                    tuple: c.tuple.clone(),
                    diff: -c.diff,
                })
                .collect();
            coll.apply_changes(negated);
        }
    }

    fn clone_box(&self) -> Box<dyn AnyChanges> {
        Box::new(self.clone())
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
