//! Type-erased traits for collections and changes.

use std::any::Any;

use crate::change::{Change, Diff};
use crate::collection::Multiset;
use crate::Tuple;

/// Type-erased collection storage.
pub trait AnyCollection: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn clone_box(&self) -> Box<dyn AnyCollection>;
    fn clear(&mut self);
    fn is_empty(&self) -> bool;
    /// Merge tuples from another collection into this one (union).
    fn merge_from(&mut self, other: &dyn AnyCollection);
    /// Compute the changes needed to go from `old` to `self`.
    /// Returns (new - old) as insertions and (old - new) as deletions.
    fn diff_from(&self, old: &dyn AnyCollection) -> Box<dyn AnyChanges>;
}

impl<T: Tuple + Send + Sync> AnyCollection for Multiset<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn AnyCollection> {
        Box::new(self.clone())
    }

    fn clear(&mut self) {
        Multiset::clear(self);
    }

    fn is_empty(&self) -> bool {
        Multiset::is_empty(self)
    }

    fn merge_from(&mut self, other: &dyn AnyCollection) {
        if let Some(other_coll) = other.as_any().downcast_ref::<Multiset<T>>() {
            for tuple in other_coll.iter() {
                self.insert(tuple.clone());
            }
        }
    }

    fn diff_from(&self, old: &dyn AnyCollection) -> Box<dyn AnyChanges> {
        let mut changes = Vec::<Change<T>>::new();

        if let Some(old_coll) = old.as_any().downcast_ref::<Multiset<T>>() {
            // Find insertions: tuples in self but not in old (or with higher multiplicity)
            for (tuple, new_mult) in self.iter_with_multiplicity() {
                let old_mult = old_coll.get(tuple);
                let diff = new_mult.0 - old_mult.0;
                if diff > 0 {
                    changes.push(Change::new(tuple.clone(), Diff(diff)));
                }
            }
            // Find deletions: tuples in old but not in self (or with lower multiplicity)
            for (tuple, old_mult) in old_coll.iter_with_multiplicity() {
                let new_mult = self.get(tuple);
                let diff = old_mult.0 - new_mult.0;
                if diff > 0 {
                    changes.push(Change::new(tuple.clone(), Diff(-diff)));
                }
            }
        }

        Box::new(changes)
    }
}

/// Type-erased change batch.
pub trait AnyChanges: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn is_empty(&self) -> bool;
    fn len(&self) -> usize;
    fn clear(&mut self);
    /// Create an empty version of the same type.
    fn clone_empty(&self) -> Box<dyn AnyChanges>;
}

impl<T: Tuple + Send + Sync> AnyChanges for Vec<Change<T>> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn clear(&mut self) {
        Vec::clear(self);
    }

    fn clone_empty(&self) -> Box<dyn AnyChanges> {
        Box::new(Vec::<Change<T>>::new())
    }
}

/// A type-erased operator function.
pub type OperatorFn = Box<
    dyn Fn(&[&dyn Any], &dyn AnyCollection) -> (Box<dyn AnyCollection>, Box<dyn AnyChanges>)
        + Send
        + Sync,
>;

/// A type-erased incremental operator function.
/// Takes: input changes, input states, output state -> output changes
pub type IncrementalOpFn = Box<
    dyn Fn(&[&dyn AnyChanges], &[&dyn AnyCollection], &dyn AnyCollection) -> Box<dyn AnyChanges>
        + Send
        + Sync,
>;
