//! The main Database type that orchestrates the relational query engine.

use std::sync::Arc;

use crate::change::{Change, Diff};
use crate::checkpoint::{Checkpoint, CheckpointId, CheckpointManager, CheckpointStack, RestoreInfo};
use crate::collection::Collection;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph, NodeId};
use crate::operators;
use crate::relation::{Relation, Variable};
use crate::Tuple;

/// A type-erased recomputation function that reads input states and produces a new output.
type RecomputeFn = Box<dyn Fn(&DataflowGraph) -> Box<dyn AnyCollection> + Send + Sync>;

/// Data needed for feedback rollback during pop().
struct FeedbackRollbackData {
    /// Index into feedback_loops.
    index: usize,
    /// The variable node ID.
    var_id: NodeId,
    /// Outputs that were added during this checkpoint frame.
    outputs: Option<Box<dyn AnyCollection>>,
    /// Input deltas that were added to input_totals during this checkpoint frame.
    input_deltas: Option<Box<dyn AnyCollection>>,
}

/// The main database type managing relations and queries.
pub struct Database {
    graph: DataflowGraph,
    checkpoints: CheckpointManager,
    /// Stack-based checkpoints for backtracking (push/pop semantics).
    checkpoint_stack: CheckpointStack,
    /// Maximum iterations for fixed-point computation.
    max_iterations: usize,
    /// Type-erased recomputation functions for each derived node.
    recompute_fns: Vec<Option<RecomputeFn>>,
    /// Feedback loops in order of declaration (for stratified fixpoint).
    /// Each entry is (variable_id, base_id, recursive_id, recompute_fn).
    feedback_loops: Vec<FeedbackLoop>,
}


/// A feedback loop for stratified fixpoint computation.
struct FeedbackLoop {
    /// The variable node that receives feedback.
    var_id: NodeId,
    /// Type-erased function to compute the input state (what's flowing into the feedback).
    /// Returns distinct(union(base, recursive)).
    compute_input: RecomputeFn,
    /// Cumulative input multiplicities (persists across all checkpoints).
    /// This is a Collection<T> storing the sum of all multiplicities ever received.
    input_totals: Box<dyn AnyCollection>,
    /// Type-erased operations for this feedback's tuple type.
    ops: Box<dyn FeedbackOps + Send + Sync>,
}

/// Type-erased operations for a feedback loop.
trait FeedbackOps: Send + Sync {
    /// Update input_totals by adding the given input's multiplicities.
    /// Returns tuples that are newly positive (went from <=0 to >0).
    fn add_to_input_totals(
        &self,
        input_totals: &mut dyn AnyCollection,
        input: &dyn AnyCollection,
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
    fn apply_output_adds(
        &self,
        output: &mut dyn AnyCollection,
        tuples: &dyn AnyCollection,
    );

    /// Apply output removals (-1 for each tuple in the collection).
    fn apply_output_removes(
        &self,
        output: &mut dyn AnyCollection,
        tuples: &dyn AnyCollection,
    );

    /// Clone the tuples collection for storage in checkpoint frame.
    fn clone_tuples(&self, tuples: &dyn AnyCollection) -> Box<dyn AnyCollection>;
}

/// Concrete implementation of FeedbackOps for a specific tuple type.
struct TypedFeedbackOps<T: Tuple + Send + Sync> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Tuple + Send + Sync> TypedFeedbackOps<T> {
    fn new() -> Self {
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
    ) -> Box<dyn AnyCollection> {
        let totals = input_totals.as_any_mut().downcast_mut::<Collection<T>>().unwrap();
        let input_coll = input.as_any().downcast_ref::<Collection<T>>().unwrap();

        let mut newly_positive = Collection::<T>::new();

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
        let totals = input_totals.as_any_mut().downcast_mut::<Collection<T>>().unwrap();
        let input_coll = input.as_any().downcast_ref::<Collection<T>>().unwrap();

        for (tuple, diff) in input_coll.iter_with_multiplicity() {
            totals.apply_change(Change::new(tuple.clone(), -diff));
        }
    }

    fn get_positive_in_totals(
        &self,
        input_totals: &dyn AnyCollection,
        input: &dyn AnyCollection,
    ) -> Box<dyn AnyCollection> {
        let totals = input_totals.as_any().downcast_ref::<Collection<T>>().unwrap();
        let input_coll = input.as_any().downcast_ref::<Collection<T>>().unwrap();

        let mut positive = Collection::<T>::new();
        for (tuple, _) in input_coll.iter_with_multiplicity() {
            if totals.get(tuple).is_positive() {
                positive.insert(tuple.clone());
            }
        }

        Box::new(positive)
    }

    fn apply_output_adds(
        &self,
        output: &mut dyn AnyCollection,
        tuples: &dyn AnyCollection,
    ) {
        let out = output.as_any_mut().downcast_mut::<Collection<T>>().unwrap();
        let tuples_coll = tuples.as_any().downcast_ref::<Collection<T>>().unwrap();

        for tuple in tuples_coll.iter() {
            out.insert(tuple.clone());
        }
    }

    fn apply_output_removes(
        &self,
        output: &mut dyn AnyCollection,
        tuples: &dyn AnyCollection,
    ) {
        let out = output.as_any_mut().downcast_mut::<Collection<T>>().unwrap();
        let tuples_coll = tuples.as_any().downcast_ref::<Collection<T>>().unwrap();

        for tuple in tuples_coll.iter() {
            out.delete(tuple.clone());
        }
    }

    fn clone_tuples(&self, tuples: &dyn AnyCollection) -> Box<dyn AnyCollection> {
        tuples.clone_box()
    }
}

impl Database {
    /// Create a new empty database.
    pub fn new() -> Self {
        Database {
            graph: DataflowGraph::new(),
            checkpoints: CheckpointManager::new(),
            checkpoint_stack: CheckpointStack::new(),
            max_iterations: 1000,
            recompute_fns: Vec::new(),
            feedback_loops: Vec::new(),
        }
    }

    /// Set the maximum iterations for fixed-point computation.
    pub fn set_max_iterations(&mut self, max: usize) {
        self.max_iterations = max;
    }

    fn ensure_recompute_fns_len(&mut self, id: NodeId) {
        while self.recompute_fns.len() <= id.index() {
            self.recompute_fns.push(None);
        }
    }

    // ========================================================================
    // Input Relations
    // ========================================================================

    /// Create a new input relation.
    /// Changes to this relation are recorded and undone on pop().
    pub fn create_input<T: Tuple + Send + Sync>(&mut self, name: &str) -> Relation<T> {
        let id = self.graph.create_input::<T>(name);
        self.ensure_recompute_fns_len(id);
        Relation::new(id)
    }

    /// Create a new persistent input relation.
    /// Changes to this relation are NOT recorded and survive pop().
    /// Use this for data that should persist across backtracking, like learned clauses in a SAT solver.
    pub fn create_persistent_input<T: Tuple + Send + Sync>(&mut self, name: &str) -> Relation<T> {
        let id = self.graph.create_persistent_input::<T>(name);
        self.ensure_recompute_fns_len(id);
        Relation::new(id)
    }

    /// Insert a tuple into a relation.
    pub fn insert<T: Tuple + Send + Sync>(&mut self, rel: Relation<T>, tuple: T) {
        // Record the change for checkpoint stack if recording (skip persistent inputs)
        if self.checkpoint_stack.is_recording() && !self.graph.get(rel.id).is_persistent() {
            self.checkpoint_stack.record(rel.id, vec![Change::insert(tuple.clone())]);
        }

        let node = self.graph.get_mut(rel.id);

        if let Some(changes) = node.pending_changes.as_any_mut().downcast_mut::<Vec<Change<T>>>() {
            changes.push(Change::insert(tuple.clone()));
        }

        if let Some(coll) = node.state.as_any_mut().downcast_mut::<Collection<T>>() {
            coll.insert(tuple);
        }

        self.graph.mark_dirty(rel.id);
        self.propagate_changes();
    }

    /// Delete a tuple from a relation.
    pub fn delete<T: Tuple + Send + Sync>(&mut self, rel: Relation<T>, tuple: T) {
        // Record the change for checkpoint stack if recording (skip persistent inputs)
        if self.checkpoint_stack.is_recording() && !self.graph.get(rel.id).is_persistent() {
            self.checkpoint_stack.record(rel.id, vec![Change::delete(tuple.clone())]);
        }

        let node = self.graph.get_mut(rel.id);

        if let Some(changes) = node.pending_changes.as_any_mut().downcast_mut::<Vec<Change<T>>>() {
            changes.push(Change::delete(tuple.clone()));
        }

        if let Some(coll) = node.state.as_any_mut().downcast_mut::<Collection<T>>() {
            coll.delete(tuple);
        }

        self.graph.mark_dirty(rel.id);
        self.propagate_changes();
    }

    /// Propagate changes through the dataflow graph.
    fn propagate_changes(&mut self) {
        if self.feedback_loops.is_empty() {
            // No feedback loops - just recompute derived relations
            self.recompute_all();
            self.graph.clear_dirty();
        } else {
            // Re-run stratified fixpoint to propagate through feedback loops
            self.run_stratified_fixpoint();
        }
    }

    // ========================================================================
    // Querying
    // ========================================================================

    /// Iterate over tuples in a relation.
    pub fn iter<T: Tuple + Send + Sync>(
        &self,
        rel: Relation<T>,
    ) -> impl Iterator<Item = &T> {
        self.graph
            .get(rel.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .into_iter()
            .flat_map(|c| c.iter())
    }

    /// Collect relation contents into a Vec.
    pub fn collect<T: Tuple + Send + Sync>(&self, rel: Relation<T>) -> Vec<T> {
        self.iter(rel).cloned().collect()
    }

    /// Iterate over tuples with their multiplicities.
    ///
    /// This allows viewing the differential state of a relation,
    /// showing how many times each tuple appears.
    pub fn iter_with_multiplicity<T: Tuple + Send + Sync>(
        &self,
        rel: Relation<T>,
    ) -> impl Iterator<Item = (&T, Diff)> {
        self.graph
            .get(rel.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .into_iter()
            .flat_map(|c| c.iter_with_multiplicity())
    }

    /// Get the multiplicity of a specific tuple in a relation.
    pub fn multiplicity<T: Tuple + Send + Sync>(&self, rel: Relation<T>, tuple: &T) -> Diff {
        self.graph
            .get(rel.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .map(|c| c.get(tuple))
            .unwrap_or(Diff::ZERO)
    }

    // ========================================================================
    // Relational Operators
    // ========================================================================

    /// Map: transform each tuple.
    pub fn map<T, U, F>(&mut self, input: Relation<T>, f: F) -> Relation<U>
    where
        T: Tuple + Send + Sync,
        U: Tuple + Send + Sync,
        F: Fn(&T) -> U + Send + Sync + 'static,
    {
        let f = Arc::new(f);
        let f_recompute = f.clone();
        let input_id = input.id;

        let id = self.graph.create_derived::<U>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<U>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<U>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::map(&input_coll, |t| f_recompute(t))) as Box<dyn AnyCollection>
        }));

        // Compute initial state
        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::map(&input_coll, |t| f(t));
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    /// Filter: keep tuples matching predicate.
    pub fn filter<T, F>(&mut self, input: Relation<T>, pred: F) -> Relation<T>
    where
        T: Tuple + Send + Sync,
        F: Fn(&T) -> bool + Send + Sync + 'static,
    {
        let pred = Arc::new(pred);
        let pred_recompute = pred.clone();
        let input_id = input.id;

        let id = self.graph.create_derived::<T>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<T>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<T>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::filter(&input_coll, |t| pred_recompute(t))) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::filter(&input_coll, |t| pred(t));
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    /// FlatMap: transform each tuple into zero or more tuples.
    pub fn flat_map<T, U, I, F>(&mut self, input: Relation<T>, f: F) -> Relation<U>
    where
        T: Tuple + Send + Sync,
        U: Tuple + Send + Sync,
        I: IntoIterator<Item = U>,
        F: Fn(&T) -> I + Send + Sync + 'static,
    {
        let f = Arc::new(f);
        let f_recompute = f.clone();
        let input_id = input.id;

        let id = self.graph.create_derived::<U>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<U>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<U>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            let mut output = Collection::<U>::new();
            for t in input_coll.iter() {
                for u in f_recompute(t) {
                    output.insert(u);
                }
            }
            Box::new(output) as Box<dyn AnyCollection>
        }));

        // Compute initial state
        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();

        let mut output = Collection::<U>::new();
        for t in input_coll.iter() {
            for u in f(t) {
                output.insert(u);
            }
        }
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    /// Join two relations on a key.
    ///
    /// Produces tuples `(left_tuple, right_tuple)` for all matching keys.
    pub fn join<L, R, K, FL, FR>(
        &mut self,
        left: Relation<L>,
        right: Relation<R>,
        key_left: FL,
        key_right: FR,
    ) -> Relation<(L, R)>
    where
        L: Tuple + Send + Sync,
        R: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        FL: Fn(&L) -> K + Send + Sync + 'static,
        FR: Fn(&R) -> K + Send + Sync + 'static,
    {
        let key_left = Arc::new(key_left);
        let key_right = Arc::new(key_right);
        let kl_recompute = key_left.clone();
        let kr_recompute = key_right.clone();
        let left_id = left.id;
        let right_id = right.id;

        let id = self.graph.create_derived::<(L, R)>(
            None,
            vec![left.id, right.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<(L, R)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(L, R)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let left_coll = graph
                .get(left_id)
                .state
                .as_any()
                .downcast_ref::<Collection<L>>()
                .cloned()
                .unwrap_or_default();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Collection<R>>()
                .cloned()
                .unwrap_or_default();
            let output = operators::join(&left_coll, &right_coll, |l| kl_recompute(l), |r| kr_recompute(r));
            Box::new(output) as Box<dyn AnyCollection>
        }));

        // Compute initial state
        let left_coll = self
            .graph
            .get(left.id)
            .state
            .as_any()
            .downcast_ref::<Collection<L>>()
            .cloned()
            .unwrap_or_default();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Collection<R>>()
            .cloned()
            .unwrap_or_default();

        let output = operators::join(&left_coll, &right_coll, |l| key_left(l), |r| key_right(r));
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    /// Union: combine two relations.
    pub fn union<T>(&mut self, left: Relation<T>, right: Relation<T>) -> Relation<T>
    where
        T: Tuple + Send + Sync,
    {
        let left_id = left.id;
        let right_id = right.id;

        let id = self.graph.create_derived::<T>(
            None,
            vec![left.id, right.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<T>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<T>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let left_coll = graph
                .get(left_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::union(&left_coll, &right_coll)) as Box<dyn AnyCollection>
        }));

        let left_coll = self
            .graph
            .get(left.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();

        let output = operators::union(&left_coll, &right_coll);
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    /// Distinct: remove duplicate tuples (ensure multiplicity = 1).
    pub fn distinct<T>(&mut self, input: Relation<T>) -> Relation<T>
    where
        T: Tuple + Send + Sync,
    {
        let input_id = input.id;

        let id = self.graph.create_derived::<T>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<T>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<T>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::distinct(&input_coll)) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::distinct(&input_coll);
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    /// Difference: tuples in left but not in right.
    pub fn difference<T>(&mut self, left: Relation<T>, right: Relation<T>) -> Relation<T>
    where
        T: Tuple + Send + Sync,
    {
        let left_id = left.id;
        let right_id = right.id;

        let id = self.graph.create_derived::<T>(
            None,
            vec![left.id, right.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<T>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<T>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let left_coll = graph
                .get(left_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            // Compute L - R
            let mut output = left_coll.clone();
            for (t, diff) in right_coll.iter_with_multiplicity() {
                output.apply_change(Change::new(t.clone(), Diff(-diff.0)));
            }
            output.compact();
            Box::new(operators::distinct(&output)) as Box<dyn AnyCollection>
        }));

        let left_coll = self
            .graph
            .get(left.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();

        // Compute L - R
        let mut output = left_coll.clone();
        for (t, diff) in right_coll.iter_with_multiplicity() {
            output.apply_change(Change::new(t.clone(), Diff(-diff.0)));
        }
        output.compact();
        let output = operators::distinct(&output);

        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    // ========================================================================
    // Aggregation
    // ========================================================================

    /// Group by key and compute maximum value.
    ///
    /// For each distinct key K, outputs (K, max(V)) where V are all values
    /// associated with that key.
    pub fn group_max<T, K, V, FK, FV>(
        &mut self,
        input: Relation<T>,
        key_fn: FK,
        value_fn: FV,
    ) -> Relation<(K, V)>
    where
        T: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        V: Tuple + Ord + Send + Sync,
        FK: Fn(&T) -> K + Send + Sync + Clone + 'static,
        FV: Fn(&T) -> V + Send + Sync + Clone + 'static,
    {
        let input_id = input.id;
        let key_fn_clone = key_fn.clone();
        let value_fn_clone = value_fn.clone();

        let id = self.graph.create_derived::<(K, V)>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<(K, V)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(K, V)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            let output = operators::aggregate(
                &input_coll,
                &key_fn_clone,
                &value_fn_clone,
                |k, vals| operators::max(k, vals),
            );
            // Filter out None results and unwrap
            let mut result = Collection::new();
            for (opt, diff) in output.iter_with_multiplicity() {
                if let Some(kv) = opt {
                    result.apply_change(Change::new(kv.clone(), diff));
                }
            }
            Box::new(result) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::aggregate(&input_coll, &key_fn, &value_fn, |k, vals| {
            operators::max(k, vals)
        });
        // Filter out None results
        let mut result = Collection::new();
        for (opt, diff) in output.iter_with_multiplicity() {
            if let Some(kv) = opt {
                result.apply_change(Change::new(kv.clone(), diff));
            }
        }
        self.graph.get_mut(id).state = Box::new(result);

        Relation::new(id)
    }

    /// Group by key and compute minimum value.
    ///
    /// For each distinct key K, outputs (K, min(V)) where V are all values
    /// associated with that key.
    pub fn group_min<T, K, V, FK, FV>(
        &mut self,
        input: Relation<T>,
        key_fn: FK,
        value_fn: FV,
    ) -> Relation<(K, V)>
    where
        T: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        V: Tuple + Ord + Send + Sync,
        FK: Fn(&T) -> K + Send + Sync + Clone + 'static,
        FV: Fn(&T) -> V + Send + Sync + Clone + 'static,
    {
        let input_id = input.id;
        let key_fn_clone = key_fn.clone();
        let value_fn_clone = value_fn.clone();

        let id = self.graph.create_derived::<(K, V)>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<(K, V)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(K, V)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            let output = operators::aggregate(
                &input_coll,
                &key_fn_clone,
                &value_fn_clone,
                |k, vals| operators::min(k, vals),
            );
            // Filter out None results and unwrap
            let mut result = Collection::new();
            for (opt, diff) in output.iter_with_multiplicity() {
                if let Some(kv) = opt {
                    result.apply_change(Change::new(kv.clone(), diff));
                }
            }
            Box::new(result) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::aggregate(&input_coll, &key_fn, &value_fn, |k, vals| {
            operators::min(k, vals)
        });
        // Filter out None results
        let mut result = Collection::new();
        for (opt, diff) in output.iter_with_multiplicity() {
            if let Some(kv) = opt {
                result.apply_change(Change::new(kv.clone(), diff));
            }
        }
        self.graph.get_mut(id).state = Box::new(result);

        Relation::new(id)
    }

    /// Group by key and compute sum of values.
    ///
    /// For each distinct key K, outputs (K, sum(V)) where V are all i64 values
    /// associated with that key, weighted by multiplicity.
    pub fn group_sum<T, K, FK, FV>(
        &mut self,
        input: Relation<T>,
        key_fn: FK,
        value_fn: FV,
    ) -> Relation<(K, i64)>
    where
        T: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        FK: Fn(&T) -> K + Send + Sync + Clone + 'static,
        FV: Fn(&T) -> i64 + Send + Sync + Clone + 'static,
    {
        let input_id = input.id;
        let key_fn_clone = key_fn.clone();
        let value_fn_clone = value_fn.clone();

        let id = self.graph.create_derived::<(K, i64)>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<(K, i64)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(K, i64)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::aggregate(
                &input_coll,
                &key_fn_clone,
                &value_fn_clone,
                |k, vals| operators::sum(k, vals),
            )) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::aggregate(&input_coll, &key_fn, &value_fn, |k, vals| {
            operators::sum(k, vals)
        });
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    /// Group by key and count tuples.
    ///
    /// For each distinct key K, outputs (K, count) where count is the number
    /// of tuples with that key (weighted by multiplicity).
    pub fn group_count<T, K, FK>(
        &mut self,
        input: Relation<T>,
        key_fn: FK,
    ) -> Relation<(K, i64)>
    where
        T: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        FK: Fn(&T) -> K + Send + Sync + Clone + 'static,
    {
        let input_id = input.id;
        let key_fn_clone = key_fn.clone();

        let id = self.graph.create_derived::<(K, i64)>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Collection::<(K, i64)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(K, i64)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Collection<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::aggregate(
                &input_coll,
                &key_fn_clone,
                |_| (),
                |k, vals| operators::count(k, vals),
            )) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Collection<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::aggregate(&input_coll, &key_fn, |_| (), |k, vals| {
            operators::count(k, vals)
        });
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    /// Compute the maximum value in a relation (convenience method).
    ///
    /// Returns a relation containing at most one tuple: the maximum value.
    pub fn max<T>(&mut self, input: Relation<T>) -> Relation<T>
    where
        T: Tuple + Ord + Send + Sync,
    {
        // Use group_max with a constant key, then project out just the value
        let with_key = self.group_max(input, |_| (), |t| t.clone());
        self.map(with_key, |(_, v)| v.clone())
    }

    /// Compute the minimum value in a relation (convenience method).
    ///
    /// Returns a relation containing at most one tuple: the minimum value.
    pub fn min<T>(&mut self, input: Relation<T>) -> Relation<T>
    where
        T: Tuple + Ord + Send + Sync,
    {
        // Use group_min with a constant key, then project out just the value
        let with_key = self.group_min(input, |_| (), |t| t.clone());
        self.map(with_key, |(_, v)| v.clone())
    }

    // ========================================================================
    // Feedback / Fixed-Point
    // ========================================================================

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

        self.feedback_loops.push(FeedbackLoop {
            var_id,
            compute_input: Box::new(move |graph: &DataflowGraph| {
                let base_coll = graph
                    .get(base_id)
                    .state
                    .as_any()
                    .downcast_ref::<Collection<T>>()
                    .cloned()
                    .unwrap_or_default();

                let recursive_coll = graph
                    .get(recursive_id)
                    .state
                    .as_any()
                    .downcast_ref::<Collection<T>>()
                    .cloned()
                    .unwrap_or_default();

                // Compute the input: distinct(union(base, recursive))
                Box::new(operators::distinct(&operators::union(&base_coll, &recursive_coll)))
                    as Box<dyn AnyCollection>
            }),
            input_totals: Box::new(Collection::<T>::new()),
            ops: Box::new(TypedFeedbackOps::<T>::new()),
        });

        // Run stratified fixpoint for all feedback loops
        self.run_stratified_fixpoint();
    }

    /// Run stratified fixpoint computation for all feedback loops.
    ///
    /// Feedbacks are processed in declaration order. If any feedback produces
    /// a change, we restart from the first feedback. This continues until
    /// a full pass through all feedbacks produces no changes.
    fn run_stratified_fixpoint(&mut self) {
        // First recompute all derived nodes to reflect any input changes
        self.recompute_all();

        let mut iterations = 0;

        'outer: loop {
            if iterations >= self.max_iterations {
                panic!(
                    "Stratified fixpoint exceeded max_iterations ({}) - possible infinite loop or non-convergent feedback",
                    self.max_iterations
                );
            }

            for i in 0..self.feedback_loops.len() {
                // Compute what's flowing into this feedback
                let input = (self.feedback_loops[i].compute_input)(&self.graph);

                // Update input_totals and get newly positive tuples
                let (newly_positive, var_id) = {
                    let fl = &mut self.feedback_loops[i];
                    let np = fl.ops.add_to_input_totals(
                        fl.input_totals.as_mut(),
                        input.as_ref(),
                    );
                    (np, fl.var_id)
                };

                if !newly_positive.is_empty() {
                    // Record input deltas and output additions for checkpoint stack
                    if self.checkpoint_stack.is_recording() {
                        // Record what we added to input_totals (the newly positive tuples)
                        self.checkpoint_stack.record_feedback_input_deltas(
                            var_id,
                            self.feedback_loops[i].ops.clone_tuples(newly_positive.as_ref()),
                        );
                        // Record output additions (same as newly positive)
                        self.checkpoint_stack.record_feedback_outputs(
                            var_id,
                            self.feedback_loops[i].ops.clone_tuples(newly_positive.as_ref()),
                        );
                    }

                    // Apply +1 to output for each newly positive tuple
                    self.feedback_loops[i].ops.apply_output_adds(
                        self.graph.get_mut(var_id).state.as_mut(),
                        newly_positive.as_ref(),
                    );

                    self.recompute_all();
                    iterations += 1;
                    continue 'outer;
                }
            }

            break;
        }

        self.graph.clear_dirty();
    }

    fn recompute_all(&mut self) {
        let topo_order: Vec<NodeId> = self.graph.topo_order().to_vec();

        for &node_id in &topo_order {
            // Skip nodes without a recompute function (inputs, feedback nodes)
            if node_id.index() >= self.recompute_fns.len() {
                continue;
            }
            if let Some(ref recompute_fn) = self.recompute_fns[node_id.index()] {
                let new_state = recompute_fn(&self.graph);
                self.graph.get_mut(node_id).state = new_state;
            }
        }
    }

    // ========================================================================
    // Checkpoints
    // ========================================================================

    /// Create a checkpoint of the current state.
    pub fn checkpoint(&mut self, name: Option<&str>) -> CheckpointId {
        let id = self.checkpoints.next_id();
        let mut checkpoint = Checkpoint::new(id, name.map(|s| s.to_string()));

        for node_id in self.graph.node_ids() {
            let node = self.graph.get(node_id);
            checkpoint.states.insert(node_id, node.state.clone_box());

            if node.is_manual_input {
                checkpoint.manual_inputs.push(node_id);
            }
        }

        self.checkpoints.store(checkpoint);
        id
    }

    /// Restore to a checkpoint.
    pub fn restore(&mut self, checkpoint_id: CheckpointId) -> Option<RestoreInfo> {
        let checkpoint = self.checkpoints.get(checkpoint_id)?.clone();
        let mut info = RestoreInfo::new();

        let mut to_restore: Vec<(NodeId, Box<dyn AnyCollection>)> = Vec::new();

        for (&node_id, saved_state) in &checkpoint.states {
            let node = self.graph.get(node_id);

            if node.is_manual_input {
                info.manual_input_nodes.push(node_id);
            } else {
                to_restore.push((node_id, saved_state.clone_box()));
                info.restored_nodes.push(node_id);
            }
        }

        for (node_id, saved_state) in to_restore {
            let node = self.graph.get_mut(node_id);
            node.state = saved_state;
            self.graph.mark_dirty(node_id);
        }

        info.needs_propagation = !info.restored_nodes.is_empty();
        Some(info)
    }

    /// List all checkpoints.
    pub fn list_checkpoints(&self) -> Vec<(CheckpointId, Option<&str>)> {
        self.checkpoints
            .list()
            .iter()
            .map(|c| (c.id, c.name.as_deref()))
            .collect()
    }

    // ========================================================================
    // Stack-based Checkpoints (Push/Pop)
    // ========================================================================

    /// Push a new checkpoint frame onto the stack.
    ///
    /// Changes made after this call will be tracked and can be undone with `pop()`.
    /// Returns the new stack depth.
    pub fn push(&mut self, name: Option<&str>) -> usize {
        self.checkpoint_stack.push(name.map(|s| s.to_string()))
    }

    /// Pop the top checkpoint frame, undoing all changes since the matching push.
    ///
    /// Algorithm:
    /// 1. Unapply all input changes
    /// 2. For all feedbacks: send -1 for each tuple in this frame's output_additions
    /// 3. For each feedback in stratified order:
    ///    - Recompute derived nodes
    ///    - Subtract the computed input from input_totals
    ///    - For tuples where input_totals is still positive AND we just sent -1:
    ///      re-send +1 and record in parent frame
    ///    - Run fixpoint for feedbacks 0..=i
    ///
    /// Returns true if a frame was popped, false if the stack was empty.
    pub fn pop(&mut self) -> bool {
        let frame = match self.checkpoint_stack.pop() {
            Some(f) => f,
            None => return false,
        };

        if !frame.has_changes() {
            return true;
        }

        // Step 1: Unapply all input changes
        for node_id in frame.changed_input_nodes() {
            if let Some(changes) = frame.get(node_id) {
                changes.unapply(self.graph.get_mut(node_id).state.as_mut());
            }
        }

        // Step 2: For all feedbacks, send -1 for outputs AND subtract recorded input deltas
        // Collect the feedback data we need to process
        let feedback_data: Vec<FeedbackRollbackData> = self
            .feedback_loops
            .iter()
            .enumerate()
            .map(|(i, fl)| FeedbackRollbackData {
                index: i,
                var_id: fl.var_id,
                outputs: frame.get_feedback_outputs(fl.var_id).map(|o| o.clone_box()),
                input_deltas: frame.get_feedback_input_deltas(fl.var_id).map(|d| d.clone_box()),
            })
            .collect();

        // Apply -1 for each output we recorded, and subtract recorded input deltas from input_totals
        for data in &feedback_data {
            if let Some(outputs) = &data.outputs {
                self.feedback_loops[data.index].ops.apply_output_removes(
                    self.graph.get_mut(data.var_id).state.as_mut(),
                    outputs.as_ref(),
                );
            }
            if let Some(input_deltas) = &data.input_deltas {
                let fl = &mut self.feedback_loops[data.index];
                fl.ops.subtract_from_input_totals(
                    fl.input_totals.as_mut(),
                    input_deltas.as_ref(),
                );
            }
        }

        // Step 3: Recompute derived nodes and run fixpoint
        // After subtracting the recorded input deltas, input_totals reflects the pre-push state
        self.recompute_all();

        // Step 4: For each feedback, check if any removed tuples should be re-added
        // (because input_totals is still positive for them after the subtraction)
        for data in &feedback_data {
            if let Some(outputs) = &data.outputs {
                let still_positive = self.feedback_loops[data.index].ops.get_positive_in_totals(
                    self.feedback_loops[data.index].input_totals.as_ref(),
                    outputs.as_ref(),
                );

                if !still_positive.is_empty() {
                    // Re-add these tuples to output
                    self.feedback_loops[data.index].ops.apply_output_adds(
                        self.graph.get_mut(data.var_id).state.as_mut(),
                        still_positive.as_ref(),
                    );

                    // Record in parent frame (if exists)
                    self.checkpoint_stack.record_feedback_outputs_to_parent(
                        data.var_id,
                        self.feedback_loops[data.index].ops.clone_tuples(still_positive.as_ref()),
                    );
                }
            }
        }

        // Step 5: Run fixpoint to handle any corrections
        if !self.feedback_loops.is_empty() {
            self.run_partial_stratified_fixpoint(self.feedback_loops.len());
        }

        true
    }

    /// Run stratified fixpoint for the first `num_feedbacks` feedback loops.
    fn run_partial_stratified_fixpoint(&mut self, num_feedbacks: usize) {
        self.recompute_all();

        let mut iterations = 0;

        'outer: loop {
            if iterations >= self.max_iterations {
                panic!(
                    "Partial stratified fixpoint exceeded max_iterations ({}) - possible infinite loop",
                    self.max_iterations
                );
            }

            for i in 0..num_feedbacks.min(self.feedback_loops.len()) {
                // Compute what's flowing into this feedback
                let input = (self.feedback_loops[i].compute_input)(&self.graph);

                // Update input_totals and get newly positive tuples
                let (newly_positive, var_id) = {
                    let fl = &mut self.feedback_loops[i];
                    let np = fl.ops.add_to_input_totals(
                        fl.input_totals.as_mut(),
                        input.as_ref(),
                    );
                    (np, fl.var_id)
                };

                if !newly_positive.is_empty() {
                    // Record input deltas and output additions for checkpoint stack
                    if self.checkpoint_stack.is_recording() {
                        // Record what we added to input_totals (the newly positive tuples)
                        self.checkpoint_stack.record_feedback_input_deltas(
                            var_id,
                            self.feedback_loops[i].ops.clone_tuples(newly_positive.as_ref()),
                        );
                        // Record output additions (same as newly positive)
                        self.checkpoint_stack.record_feedback_outputs(
                            var_id,
                            self.feedback_loops[i].ops.clone_tuples(newly_positive.as_ref()),
                        );
                    }

                    // Apply +1 to output for each newly positive tuple
                    self.feedback_loops[i].ops.apply_output_adds(
                        self.graph.get_mut(var_id).state.as_mut(),
                        newly_positive.as_ref(),
                    );

                    self.recompute_all();
                    iterations += 1;
                    continue 'outer;
                }
            }

            break;
        }

        self.graph.clear_dirty();
    }

    /// Check if we're currently recording changes (have at least one frame on the stack).
    pub fn is_recording(&self) -> bool {
        self.checkpoint_stack.is_recording()
    }

    /// Get the current checkpoint stack depth.
    pub fn stack_depth(&self) -> usize {
        self.checkpoint_stack.depth()
    }

    // ========================================================================
    // Utilities
    // ========================================================================

    /// Get a relation by name.
    pub fn get<T: Tuple>(&self, name: &str) -> Option<Relation<T>> {
        self.graph.get_id(name).map(Relation::new)
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_insert() {
        let mut db = Database::new();
        let edges = db.create_input::<(i32, i32)>("edges");

        db.insert(edges, (1, 2));
        db.insert(edges, (2, 3));

        let result: Vec<_> = db.collect(edges);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&(1, 2)));
        assert!(result.contains(&(2, 3)));
    }

    #[test]
    fn test_map() {
        let mut db = Database::new();
        let nums = db.create_input::<i32>("nums");

        db.insert(nums, 1);
        db.insert(nums, 2);
        db.insert(nums, 3);

        let doubled = db.map(nums, |x| x * 2);

        let result: Vec<_> = db.collect(doubled);
        assert!(result.contains(&2));
        assert!(result.contains(&4));
        assert!(result.contains(&6));
    }

    #[test]
    fn test_join() {
        let mut db = Database::new();
        let edges = db.create_input::<(i32, i32)>("edges");
        let labels = db.create_input::<(i32, &str)>("labels");

        db.insert(edges, (1, 2));
        db.insert(edges, (2, 3));
        db.insert(labels, (1, "one"));
        db.insert(labels, (2, "two"));

        // Join edges with labels on the source node
        let joined = db.join(edges, labels, |(src, _)| *src, |(id, _)| *id);

        let result: Vec<_> = db.collect(joined);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_transitive_closure() {
        let mut db = Database::new();
        let edges = db.create_input::<(i32, i32)>("edges");

        // Create graph: 1->2->3->4
        db.insert(edges, (1, 2));
        db.insert(edges, (2, 3));
        db.insert(edges, (3, 4));

        // path = edges ∪ (path ⋈ edges).map(|(p, e)| (p.0, e.1))
        let (path_var, path) = db.variable::<(i32, i32)>("path");

        // Recursive case: extend paths by one edge
        // path(a, c) :- path(a, b), edge(b, c)
        let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
        let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));

        // Combine base (edges) and recursive (new_paths)
        let all_paths = db.union(edges, new_paths);

        db.feedback(path_var, edges, all_paths);

        let result: Vec<_> = db.collect(path);
        // Should have: (1,2), (2,3), (3,4), (1,3), (2,4), (1,4)
        assert!(result.contains(&(1, 2)));
        assert!(result.contains(&(2, 3)));
        assert!(result.contains(&(3, 4)));
        assert!(result.contains(&(1, 3)));
        assert!(result.contains(&(2, 4)));
        assert!(result.contains(&(1, 4)));
    }

    #[test]
    fn test_multiplicities() {
        let mut db = Database::new();
        let items = db.create_input::<i32>("items");

        // Insert duplicates
        db.insert(items, 10);
        db.insert(items, 10);
        db.insert(items, 20);

        // Check multiplicities
        assert_eq!(db.multiplicity(items, &10), Diff(2));
        assert_eq!(db.multiplicity(items, &20), Diff(1));
        assert_eq!(db.multiplicity(items, &30), Diff(0));

        // Delete one occurrence
        db.delete(items, 10);
        assert_eq!(db.multiplicity(items, &10), Diff(1));

        // iter_with_multiplicity shows all tuples
        let mults: Vec<_> = db.iter_with_multiplicity(items).collect();
        assert_eq!(mults.len(), 2); // 10 and 20
    }

    #[test]
    fn test_checkpoint_and_restore() {
        let mut db = Database::new();
        let numbers = db.create_input::<i32>("numbers");
        let doubled = db.map(numbers, |n| n * 2);

        // Initial state
        db.insert(numbers, 1);
        db.insert(numbers, 2);

        // Create checkpoint
        let cp = db.checkpoint(Some("initial"));

        // Verify checkpoint is listed
        let checkpoints = db.list_checkpoints();
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].1, Some("initial"));

        // Make changes
        db.insert(numbers, 3);
        db.delete(numbers, 1);

        // Verify current state
        assert_eq!(db.collect(numbers).len(), 2); // {2, 3}
        let doubled_result: Vec<_> = db.collect(doubled);
        assert!(doubled_result.contains(&4));
        assert!(doubled_result.contains(&6));

        // Restore checkpoint
        let info = db.restore(cp).unwrap();

        // Derived relations are restored
        assert!(!info.restored_nodes.is_empty());

        // Manual inputs are NOT auto-restored (numbers is manual input)
        assert!(!info.manual_input_nodes.is_empty());

        // The 'doubled' relation was restored to checkpoint state (2, 4)
        // But since numbers was NOT restored, the states may be inconsistent
        // until we propagate changes or fix inputs manually
    }
}
