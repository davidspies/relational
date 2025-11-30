//! Typed feedback operations for standard (non-timestamped) feedback loops.

use crate::Tuple;
use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection};

use super::commit_id::CommitId;
use super::feedback::FeedbackOps;

/// Concrete implementation of FeedbackOps for a specific tuple type.
pub(super) struct TypedFeedbackOps<T: Tuple + Send + Sync> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Tuple + Send + Sync> TypedFeedbackOps<T> {
    pub fn new() -> Self {
        TypedFeedbackOps {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T: Tuple + Send + Sync> FeedbackOps for TypedFeedbackOps<T> {
    fn add_to_input_totals(
        &self,
        input_totals: &mut dyn AnyCollection,
        input: &dyn AnyCollection,
        _commit_id: CommitId,
    ) -> Box<dyn AnyCollection> {
        let totals = input_totals
            .as_any_mut()
            .downcast_mut::<Multiset<T>>()
            .unwrap();
        let input_coll = input.as_any().downcast_ref::<Multiset<T>>().unwrap();

        let mut newly_positive = Multiset::<T>::new();

        // We only add tuples that are newly seen (not already in input_totals)
        // The input_totals acts as a "seen set" - once a tuple is seen (positive),
        // we don't increment its count again. This matches the seen-set semantics
        // where we only output +1 the first time we see a tuple.
        for (tuple, _diff) in input_coll.iter_with_multiplicity() {
            let old_total = totals.get(tuple);

            // Only add if not already positive
            if !old_total.is_positive() {
                // Add with multiplicity 1 (seen once)
                totals.insert(tuple.clone());
                newly_positive.insert(tuple.clone());
            }
        }

        Box::new(newly_positive)
    }

    fn subtract_from_input_totals(
        &self,
        input_totals: &mut dyn AnyCollection,
        input: &dyn AnyCollection,
    ) {
        let totals = input_totals
            .as_any_mut()
            .downcast_mut::<Multiset<T>>()
            .unwrap();
        let input_coll = input.as_any().downcast_ref::<Multiset<T>>().unwrap();

        for (tuple, diff) in input_coll.iter_with_multiplicity() {
            totals.apply_change(Change::new(tuple.clone(), -diff));
        }
    }

    fn get_positive_in_totals(
        &self,
        input_totals: &dyn AnyCollection,
        input: &dyn AnyCollection,
    ) -> Box<dyn AnyCollection> {
        let totals = input_totals.as_any().downcast_ref::<Multiset<T>>().unwrap();
        let input_coll = input.as_any().downcast_ref::<Multiset<T>>().unwrap();

        let mut positive = Multiset::<T>::new();
        for (tuple, _) in input_coll.iter_with_multiplicity() {
            if totals.get(tuple).is_positive() {
                positive.insert(tuple.clone());
            }
        }

        Box::new(positive)
    }

    fn apply_output_adds(&self, output: &mut dyn AnyCollection, tuples: &dyn AnyCollection) {
        let out = output.as_any_mut().downcast_mut::<Multiset<T>>().unwrap();
        let tuples_coll = tuples.as_any().downcast_ref::<Multiset<T>>().unwrap();

        for tuple in tuples_coll.iter() {
            out.insert(tuple.clone());
        }
    }

    fn apply_output_removes(&self, output: &mut dyn AnyCollection, tuples: &dyn AnyCollection) {
        let out = output.as_any_mut().downcast_mut::<Multiset<T>>().unwrap();
        let tuples_coll = tuples.as_any().downcast_ref::<Multiset<T>>().unwrap();

        for tuple in tuples_coll.iter() {
            out.delete(tuple.clone());
        }
    }

    fn clone_tuples(&self, tuples: &dyn AnyCollection) -> Box<dyn AnyCollection> {
        tuples.clone_box()
    }

    fn append_insert_changes(&self, pending: &mut dyn AnyChanges, tuples: &dyn AnyCollection) {
        let pending_vec = pending
            .as_any_mut()
            .downcast_mut::<Vec<Change<T>>>()
            .unwrap();
        let tuples_coll = tuples.as_any().downcast_ref::<Multiset<T>>().unwrap();
        for tuple in tuples_coll.iter() {
            pending_vec.push(Change::insert(tuple.clone()));
        }
    }
}
