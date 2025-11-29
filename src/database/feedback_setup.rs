//! Feedback variable and loop setup for Database.

use crate::collection::Multiset;
use crate::dataflow::{AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::{Relation, Variable};
use crate::Tuple;

use super::commit_id::CommitId;
use super::feedback::{FeedbackLoop, StratifiedOp, TimestampedFeedbackOps, TypedFeedbackOps};
use super::Database;

impl Database {
    /// Create a variable for feedback loops.
    ///
    /// Returns `(variable, relation)` where:
    /// - `variable` is used to set up feedback
    /// - `relation` is used to read from the variable in computations
    pub fn variable<T: Tuple + Send + Sync>(&mut self, name: &str) -> (Variable<T>, Relation<T>) {
        let id = self.graph.create_feedback::<T>(name);
        self.ensure_recompute_fns_len(id);
        (Variable::new(id), Relation::new(id))
    }

    /// Complete a feedback loop by connecting computed output back to variable.
    ///
    /// The variable acts as a monotonically growing "seen set":
    /// - Takes tuples with positive multiplicity from `base ∪ recursive`
    /// - Adds them to the variable with multiplicity 1 if not already present
    /// - Never removes tuples (except via pop)
    ///
    /// Feedback loops are evaluated in a stratified manner based on declaration order:
    /// - Each feedback runs to fixpoint before the next one is applied
    /// - When a later feedback changes, all earlier feedbacks re-run to fixpoint
    /// - This continues until all feedbacks reach a global fixpoint
    pub fn feedback<T: Tuple + Send + Sync>(
        &mut self,
        var: Variable<T>,
        base: Relation<T>,
        recursive: Relation<T>,
    ) {
        let node = self.graph.get_mut(var.id);
        node.inputs = vec![base.id, recursive.id];

        // Store the feedback loop info
        let var_id = var.id;
        let base_id = base.id;
        let recursive_id = recursive.id;

        self.stratified_ops
            .push(StratifiedOp::Feedback(FeedbackLoop {
                var_id,
                compute_input: Box::new(move |graph: &DataflowGraph| {
                    let base_coll = graph
                        .get(base_id)
                        .state
                        .as_any()
                        .downcast_ref::<Multiset<T>>()
                        .cloned()
                        .unwrap_or_default();

                    let recursive_coll = graph
                        .get(recursive_id)
                        .state
                        .as_any()
                        .downcast_ref::<Multiset<T>>()
                        .cloned()
                        .unwrap_or_default();

                    // Compute the input: distinct(union(base, recursive))
                    Box::new(operators::distinct(&operators::union(
                        &base_coll,
                        &recursive_coll,
                    ))) as Box<dyn AnyCollection>
                }),
                input_totals: Box::new(Multiset::<T>::new()),
                ops: Box::new(TypedFeedbackOps::<T>::new()),
            }));

        // Run stratified fixpoint for all feedback loops
        self.run_stratified_fixpoint();
    }

    /// Complete a feedback loop that tracks discovery time.
    ///
    /// Like `feedback`, but the variable holds `(T, CommitId)` where the CommitId
    /// records when each tuple was first discovered. This allows deriving relations
    /// that depend on discovery order (e.g., taking the tuple with minimum CommitId).
    ///
    /// The input `base` and `recursive` are `Relation<T>`, but the variable holds
    /// `(T, CommitId)`. When a tuple T is first seen, it's added to the variable
    /// with the current commit ID.
    pub fn feedback_with_id<T: Tuple + Send + Sync>(
        &mut self,
        var: Variable<(T, CommitId)>,
        base: Relation<T>,
        recursive: Relation<T>,
    ) {
        let node = self.graph.get_mut(var.id);
        node.inputs = vec![base.id, recursive.id];

        let var_id = var.id;
        let base_id = base.id;
        let recursive_id = recursive.id;

        self.stratified_ops
            .push(StratifiedOp::Feedback(FeedbackLoop {
                var_id,
                compute_input: Box::new(move |graph: &DataflowGraph| {
                    let base_coll = graph
                        .get(base_id)
                        .state
                        .as_any()
                        .downcast_ref::<Multiset<T>>()
                        .cloned()
                        .unwrap_or_default();

                    let recursive_coll = graph
                        .get(recursive_id)
                        .state
                        .as_any()
                        .downcast_ref::<Multiset<T>>()
                        .cloned()
                        .unwrap_or_default();

                    // Compute the input: distinct(union(base, recursive))
                    // This is Collection<T>, not Collection<(T, CommitId)>
                    Box::new(operators::distinct(&operators::union(
                        &base_coll,
                        &recursive_coll,
                    ))) as Box<dyn AnyCollection>
                }),
                // input_totals is Collection<T> (the seen set)
                input_totals: Box::new(Multiset::<T>::new()),
                // TimestampedFeedbackOps handles the T -> (T, CommitId) conversion
                ops: Box::new(TimestampedFeedbackOps::<T>::new()),
            }));

        // Run stratified fixpoint for all feedback loops
        self.run_stratified_fixpoint();
    }

    /// Add an interrupt that stops fixpoint propagation when the relation is non-empty.
    ///
    /// Interrupts are checked in declaration order along with feedbacks.
    /// When an interrupt fires (relation non-empty), the fixpoint stops immediately.
    /// Use `was_interrupted()` to check if the last fixpoint was interrupted.
    pub fn interrupt<T: Tuple + Send + Sync>(&mut self, rel: Relation<T>) {
        let rel_id = rel.id;
        self.stratified_ops.push(StratifiedOp::Interrupt {
            check: Box::new(move |graph: &DataflowGraph| {
                graph
                    .get(rel_id)
                    .state
                    .as_any()
                    .downcast_ref::<Multiset<T>>()
                    .map(|c| !c.is_empty())
                    .unwrap_or(false)
            }),
        });
    }
}
