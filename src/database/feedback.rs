//! Feedback loop infrastructure for stratified fixpoint computation.

use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph, NodeId};

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
    /// Returns distinct(input).
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
