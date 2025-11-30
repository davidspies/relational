//! Timestamped feedback operations for tracking tuple discovery times.

use crate::Tuple;
use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection};

use super::commit_id::CommitId;
use super::feedback::FeedbackOps;

/// Feedback operations for a variable that tracks discovery time.
/// The variable holds (T, CommitId) where CommitId is when T was first seen.
/// Input is T, output is (T, CommitId).
pub(super) struct TimestampedFeedbackOps<T: Tuple + Send + Sync> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Tuple + Send + Sync> TimestampedFeedbackOps<T> {
    pub fn new() -> Self {
        TimestampedFeedbackOps {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T: Tuple + Send + Sync> FeedbackOps for TimestampedFeedbackOps<T> {
    fn add_to_input_totals(
        &self,
        input_totals: &mut dyn AnyCollection,
        input: &dyn AnyCollection,
        commit_id: CommitId,
    ) -> Box<dyn AnyCollection> {
        // input_totals is Collection<T> (the seen set, just like regular feedback)
        // input is Collection<T> (new tuples to consider)
        // output is Collection<(T, CommitId)> (newly seen tuples with their discovery time)
        let totals = input_totals
            .as_any_mut()
            .downcast_mut::<Multiset<T>>()
            .unwrap();
        let input_coll = input.as_any().downcast_ref::<Multiset<T>>().unwrap();

        let mut newly_positive = Multiset::<(T, CommitId)>::new();

        for (tuple, _diff) in input_coll.iter_with_multiplicity() {
            let old_total = totals.get(tuple);

            if !old_total.is_positive() {
                totals.insert(tuple.clone());
                // Stamp with current commit ID
                newly_positive.insert((tuple.clone(), commit_id));
            }
        }

        Box::new(newly_positive)
    }

    fn subtract_from_input_totals(
        &self,
        input_totals: &mut dyn AnyCollection,
        input: &dyn AnyCollection,
    ) {
        // input_totals is Collection<T>
        // input is Collection<(T, CommitId)> (the timestamped tuples we recorded)
        let totals = input_totals
            .as_any_mut()
            .downcast_mut::<Multiset<T>>()
            .unwrap();
        let input_coll = input
            .as_any()
            .downcast_ref::<Multiset<(T, CommitId)>>()
            .unwrap();

        for ((tuple, _commit_id), diff) in input_coll.iter_with_multiplicity() {
            totals.apply_change(Change::new(tuple.clone(), -diff));
        }
    }

    fn get_positive_in_totals(
        &self,
        input_totals: &dyn AnyCollection,
        input: &dyn AnyCollection,
    ) -> Box<dyn AnyCollection> {
        // input_totals is Collection<T>
        // input is Collection<(T, CommitId)>
        // Returns the subset of input where T is still positive in totals
        let totals = input_totals.as_any().downcast_ref::<Multiset<T>>().unwrap();
        let input_coll = input
            .as_any()
            .downcast_ref::<Multiset<(T, CommitId)>>()
            .unwrap();

        let mut positive = Multiset::<(T, CommitId)>::new();
        for ((tuple, commit_id), _) in input_coll.iter_with_multiplicity() {
            if totals.get(tuple).is_positive() {
                positive.insert((tuple.clone(), *commit_id));
            }
        }

        Box::new(positive)
    }

    fn apply_output_adds(&self, output: &mut dyn AnyCollection, tuples: &dyn AnyCollection) {
        // output is Collection<(T, CommitId)>
        // tuples is Collection<(T, CommitId)>
        let out = output
            .as_any_mut()
            .downcast_mut::<Multiset<(T, CommitId)>>()
            .unwrap();
        let tuples_coll = tuples
            .as_any()
            .downcast_ref::<Multiset<(T, CommitId)>>()
            .unwrap();

        for tuple in tuples_coll.iter() {
            out.insert(tuple.clone());
        }
    }

    fn apply_output_removes(&self, output: &mut dyn AnyCollection, tuples: &dyn AnyCollection) {
        // output is Collection<(T, CommitId)>
        // tuples is Collection<(T, CommitId)>
        let out = output
            .as_any_mut()
            .downcast_mut::<Multiset<(T, CommitId)>>()
            .unwrap();
        let tuples_coll = tuples
            .as_any()
            .downcast_ref::<Multiset<(T, CommitId)>>()
            .unwrap();

        for tuple in tuples_coll.iter() {
            out.delete(tuple.clone());
        }
    }

    fn clone_tuples(&self, tuples: &dyn AnyCollection) -> Box<dyn AnyCollection> {
        tuples.clone_box()
    }

    fn append_insert_changes(&self, pending: &mut dyn AnyChanges, tuples: &dyn AnyCollection) {
        // pending is Vec<Change<(T, CommitId)>>
        // tuples is Collection<(T, CommitId)>
        let pending_vec = pending
            .as_any_mut()
            .downcast_mut::<Vec<Change<(T, CommitId)>>>()
            .unwrap();
        let tuples_coll = tuples
            .as_any()
            .downcast_ref::<Multiset<(T, CommitId)>>()
            .unwrap();
        for tuple in tuples_coll.iter() {
            pending_vec.push(Change::insert(tuple.clone()));
        }
    }
}
