//! Feedback loop infrastructure for stratified fixpoint computation.

use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph, NodeId};
use crate::Tuple;

use super::commit_id::CommitId;
use super::RecomputeFn;

/// Data needed for feedback rollback during pop().
pub(super) struct FeedbackRollbackData {
    /// Index into feedback_loops.
    pub index: usize,
    /// The variable node ID.
    pub var_id: NodeId,
    /// Outputs that were added during this checkpoint frame.
    pub outputs: Option<Box<dyn AnyCollection>>,
    /// Input deltas that were added to input_totals during this checkpoint frame.
    pub input_deltas: Option<Box<dyn AnyCollection>>,
}

/// An operation in the stratified fixpoint computation.
pub(super) enum StratifiedOp {
    /// A feedback loop that runs to fixpoint.
    Feedback(FeedbackLoop),
    /// An interrupt that stops propagation if the relation is non-empty.
    Interrupt {
        /// Function to check if the interrupt condition is met (relation non-empty).
        check: Box<dyn Fn(&DataflowGraph) -> bool + Send + Sync>,
    },
}

/// A feedback loop for stratified fixpoint computation.
pub(super) struct FeedbackLoop {
    /// The variable node that receives feedback.
    pub var_id: NodeId,
    /// Type-erased function to compute the input state (what's flowing into the feedback).
    /// Returns distinct(union(base, recursive)).
    pub compute_input: RecomputeFn,
    /// Cumulative input multiplicities (persists across all checkpoints).
    /// This is a Collection<T> storing the sum of all multiplicities ever received.
    pub input_totals: Box<dyn AnyCollection>,
    /// Type-erased operations for this feedback's tuple type.
    pub ops: Box<dyn FeedbackOps + Send + Sync>,
}

/// Type-erased operations for a feedback loop.
pub(super) trait FeedbackOps: Send + Sync {
    /// Update input_totals by adding the given input's multiplicities.
    /// Returns tuples that are newly positive (went from <=0 to >0).
    /// The commit_id is used for timestamped variants to record when tuples are first seen.
    fn add_to_input_totals(
        &self,
        input_totals: &mut dyn AnyCollection,
        input: &dyn AnyCollection,
        commit_id: CommitId,
    ) -> Box<dyn AnyCollection>;

    /// Update input_totals by subtracting the given input's multiplicities.
    fn subtract_from_input_totals(
        &self,
        input_totals: &mut dyn AnyCollection,
        input: &dyn AnyCollection,
    );

    /// Check which tuples in input have positive total in input_totals.
    fn get_positive_in_totals(
        &self,
        input_totals: &dyn AnyCollection,
        input: &dyn AnyCollection,
    ) -> Box<dyn AnyCollection>;

    /// Apply output additions (+1 for each tuple in the collection).
    fn apply_output_adds(&self, output: &mut dyn AnyCollection, tuples: &dyn AnyCollection);

    /// Apply output removals (-1 for each tuple in the collection).
    fn apply_output_removes(&self, output: &mut dyn AnyCollection, tuples: &dyn AnyCollection);

    /// Clone the tuples collection for storage in checkpoint frame.
    fn clone_tuples(&self, tuples: &dyn AnyCollection) -> Box<dyn AnyCollection>;

    /// Append insert changes to the pending changes vector.
    fn append_insert_changes(&self, pending: &mut dyn AnyChanges, tuples: &dyn AnyCollection);
}

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
