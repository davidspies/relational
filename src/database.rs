//! The main Database type that orchestrates the relational query engine.

use std::sync::Arc;

use crate::Tuple;
use crate::change::{Change, Diff};
use crate::checkpoint::{
    Checkpoint, CheckpointId, CheckpointManager, CheckpointStack, RestoreInfo,
};
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph, NodeId};
use crate::operators;
use crate::relation::{Relation, Variable};

/// Type-erased incremental operator function.
/// Takes: input node IDs, graph (for reading states and pending changes) -> output changes
/// The function is responsible for reading its inputs' states and pending changes.
type IncrementalFn =
    Box<dyn Fn(&DataflowGraph, &[&dyn AnyChanges]) -> Box<dyn AnyChanges> + Send + Sync>;

/// Type-erased function to apply changes to a node's state.
type ApplyFn = Box<dyn Fn(&mut dyn AnyCollection, &dyn AnyChanges) + Send + Sync>;

/// A monotonically increasing commit ID that tracks database mutations.
///
/// This counter is incremented:
/// - When a feedback loop produces new tuples
/// - During the global-undo step of a pop operation
///
/// The counter never decreases, even during backtracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct CommitId(u64);

impl CommitId {
    /// Create a new CommitId from a raw value.
    pub fn new(id: u64) -> Self {
        CommitId(id)
    }

    /// Get the raw u64 value.
    pub fn raw(self) -> u64 {
        self.0
    }
}

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
    /// Type-erased recomputation functions for each derived node (legacy, being phased out).
    recompute_fns: Vec<Option<RecomputeFn>>,
    /// Incremental operator functions for each derived node.
    /// Takes input changes and produces output changes.
    incremental_fns: Vec<Option<IncrementalFn>>,
    /// Type-erased apply functions for each node.
    /// These know how to apply changes to the node's state.
    apply_fns: Vec<Option<ApplyFn>>,
    /// Feedback loops and interrupts in order of declaration (for stratified fixpoint).
    stratified_ops: Vec<StratifiedOp>,
    /// Whether the last fixpoint was interrupted.
    interrupted: bool,
    /// Monotonically increasing commit counter, incremented on feedback changes and pop undo.
    commit_id: CommitId,
}

/// An operation in the stratified fixpoint computation.
enum StratifiedOp {
    /// A feedback loop that runs to fixpoint.
    Feedback(FeedbackLoop),
    /// An interrupt that stops propagation if the relation is non-empty.
    Interrupt {
        /// Function to check if the interrupt condition is met (relation non-empty).
        check: Box<dyn Fn(&DataflowGraph) -> bool + Send + Sync>,
    },
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

/// Feedback operations for a variable that tracks discovery time.
/// The variable holds (T, CommitId) where CommitId is when T was first seen.
/// Input is T, output is (T, CommitId).
struct TimestampedFeedbackOps<T: Tuple + Send + Sync> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Tuple + Send + Sync> TimestampedFeedbackOps<T> {
    fn new() -> Self {
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
            incremental_fns: Vec::new(),
            apply_fns: Vec::new(),
            stratified_ops: Vec::new(),
            interrupted: false,
            commit_id: CommitId(0),
        }
    }

    /// Get the current commit ID.
    pub fn commit_id(&self) -> CommitId {
        self.commit_id
    }

    /// Increment the commit ID and return the new value.
    fn next_commit_id(&mut self) -> CommitId {
        self.commit_id = CommitId(self.commit_id.0 + 1);
        self.commit_id
    }

    /// Set the maximum iterations for fixed-point computation.
    pub fn set_max_iterations(&mut self, max: usize) {
        self.max_iterations = max;
    }

    fn ensure_recompute_fns_len(&mut self, id: NodeId) {
        while self.recompute_fns.len() <= id.index() {
            self.recompute_fns.push(None);
        }
        while self.incremental_fns.len() <= id.index() {
            self.incremental_fns.push(None);
        }
        while self.apply_fns.len() <= id.index() {
            self.apply_fns.push(None);
        }
    }

    /// Create an apply function for a specific tuple type.
    fn make_apply_fn<T: Tuple + Send + Sync>() -> ApplyFn {
        Box::new(|state: &mut dyn AnyCollection, changes: &dyn AnyChanges| {
            if let (Some(coll), Some(changes)) = (
                state.as_any_mut().downcast_mut::<Multiset<T>>(),
                changes.as_any().downcast_ref::<Vec<Change<T>>>(),
            ) {
                coll.apply_changes(changes.iter().cloned());
            }
        })
    }

    // ========================================================================
    // Input Relations
    // ========================================================================

    /// Create a new input relation.
    /// Changes to this relation are recorded and undone on pop().
    pub fn create_input<T: Tuple + Send + Sync>(&mut self, name: &str) -> Relation<T> {
        let id = self.graph.create_input::<T>(name);
        self.ensure_recompute_fns_len(id);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<T>());
        Relation::new(id)
    }

    /// Create a new persistent input relation.
    /// Changes to this relation are NOT recorded and survive pop().
    /// Use this for data that should persist across backtracking, like learned clauses in a SAT solver.
    pub fn create_persistent_input<T: Tuple + Send + Sync>(&mut self, name: &str) -> Relation<T> {
        let id = self.graph.create_persistent_input::<T>(name);
        self.ensure_recompute_fns_len(id);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<T>());
        Relation::new(id)
    }

    /// Insert a tuple into a relation.
    ///
    /// The change is staged but not propagated until `commit()` is called.
    pub fn insert<T: Tuple + Send + Sync>(&mut self, rel: Relation<T>, tuple: T) {
        // Record the change for checkpoint stack if recording (skip persistent inputs)
        if self.checkpoint_stack.is_recording() && !self.graph.get(rel.id).is_persistent() {
            self.checkpoint_stack
                .record(rel.id, vec![Change::insert(tuple.clone())]);
        }

        let node = self.graph.get_mut(rel.id);

        if let Some(changes) = node
            .pending_changes
            .as_any_mut()
            .downcast_mut::<Vec<Change<T>>>()
        {
            changes.push(Change::insert(tuple.clone()));
        }

        if let Some(coll) = node.state.as_any_mut().downcast_mut::<Multiset<T>>() {
            coll.insert(tuple);
        }

        self.graph.mark_dirty(rel.id);
    }

    /// Delete a tuple from a relation.
    ///
    /// The change is staged but not propagated until `commit()` is called.
    pub fn delete<T: Tuple + Send + Sync>(&mut self, rel: Relation<T>, tuple: T) {
        // Record the change for checkpoint stack if recording (skip persistent inputs)
        if self.checkpoint_stack.is_recording() && !self.graph.get(rel.id).is_persistent() {
            self.checkpoint_stack
                .record(rel.id, vec![Change::delete(tuple.clone())]);
        }

        let node = self.graph.get_mut(rel.id);

        if let Some(changes) = node
            .pending_changes
            .as_any_mut()
            .downcast_mut::<Vec<Change<T>>>()
        {
            changes.push(Change::delete(tuple.clone()));
        }

        if let Some(coll) = node.state.as_any_mut().downcast_mut::<Multiset<T>>() {
            coll.delete(tuple);
        }

        self.graph.mark_dirty(rel.id);
    }

    /// Commit staged changes and propagate through the dataflow graph.
    ///
    /// This runs fixpoint computation for any feedback loops.
    pub fn commit(&mut self) {
        if self.stratified_ops.is_empty() {
            // No feedback loops or interrupts - just recompute derived relations
            self.recompute_all();
            self.graph.clear_dirty();
        } else {
            // Re-run stratified fixpoint to propagate through feedback loops
            self.run_stratified_fixpoint();
        }
    }

    /// Check if the last fixpoint computation was interrupted.
    pub fn was_interrupted(&self) -> bool {
        self.interrupted
    }

    // ========================================================================
    // Querying
    // ========================================================================

    /// Iterate over tuples in a relation.
    pub fn iter<T: Tuple + Send + Sync>(&self, rel: Relation<T>) -> impl Iterator<Item = &T> {
        self.graph
            .get(rel.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
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
            .downcast_ref::<Multiset<T>>()
            .into_iter()
            .flat_map(|c| c.iter_with_multiplicity())
    }

    /// Get the multiplicity of a specific tuple in a relation.
    pub fn multiplicity<T: Tuple + Send + Sync>(&self, rel: Relation<T>, tuple: &T) -> Diff {
        self.graph
            .get(rel.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
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
        let f_inc = f.clone();
        let f_recompute = f.clone();
        let input_id = input.id;

        let id = self.graph.create_derived::<U>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<U>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<U>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the apply function for this node's output type
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<U>());

        // Store the incremental function - map is purely local, no state needed
        self.incremental_fns[id.index()] = Some(Box::new(
            move |_graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);
                Box::new(operators::map_changes(changes, |t| f_inc(t))) as Box<dyn AnyChanges>
            },
        ));

        // Store the recompute function (for initial state and fallback)
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
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
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::map(&input_coll, |t| f(t));
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    /// Attach the current commit ID to each tuple.
    ///
    /// This transforms `T` into `(T, CommitId)`, where the CommitId is the
    /// value at the time the tuple flows through during recomputation.
    pub fn with_id<T>(&mut self, input: Relation<T>) -> Relation<(T, CommitId)>
    where
        T: Tuple + Send + Sync,
    {
        let input_id = input.id;

        let id = self.graph.create_derived::<(T, CommitId)>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<(T, CommitId)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(T, CommitId)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the apply function for this node's output type
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(T, CommitId)>());

        // Store the incremental function - stamps with current commit ID
        self.incremental_fns[id.index()] = Some(Box::new(
            move |graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);
                let commit_id = CommitId(graph.commit_id());
                Box::new(operators::map_changes(changes, |t| (t.clone(), commit_id)))
                    as Box<dyn AnyChanges>
            },
        ));

        // Store the recompute function - captures current commit ID from graph
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let commit_id = CommitId(graph.commit_id());
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();

            Box::new(operators::map(&input_coll, |t| (t.clone(), commit_id)))
                as Box<dyn AnyCollection>
        }));

        // Compute initial state
        let commit_id = self.commit_id;
        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::map(&input_coll, |t| (t.clone(), commit_id));
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
        let pred_inc = pred.clone();
        let pred_recompute = pred.clone();
        let input_id = input.id;

        let id = self.graph.create_derived::<T>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<T>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<T>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the apply function for this node's output type
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<T>());

        // Store the incremental function - filter is purely local
        self.incremental_fns[id.index()] = Some(Box::new(
            move |_graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);
                Box::new(operators::filter_changes(changes, |t| pred_inc(t))) as Box<dyn AnyChanges>
            },
        ));

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::filter(&input_coll, |t| pred_recompute(t)))
                as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
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
        let f_inc = f.clone();
        let f_recompute = f.clone();
        let input_id = input.id;

        let id = self.graph.create_derived::<U>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<U>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<U>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the apply function for this node's output type
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<U>());

        // Store the incremental function - flat_map is purely local
        self.incremental_fns[id.index()] = Some(Box::new(
            move |_graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);
                Box::new(operators::flat_map_changes(changes, |t| f_inc(t))) as Box<dyn AnyChanges>
            },
        ));

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            let mut output = Multiset::<U>::new();
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
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();

        let mut output = Multiset::<U>::new();
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
        let kl_inc = key_left.clone();
        let kr_inc = key_right.clone();
        let kl_recompute = key_left.clone();
        let kr_recompute = key_right.clone();
        let left_id = left.id;
        let right_id = right.id;

        let id = self.graph.create_derived::<(L, R)>(
            None,
            vec![left.id, right.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<(L, R)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(L, R)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the apply function for this node's output type
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(L, R)>());

        // Store the incremental function
        // Join requires state of both inputs to process changes from either side
        self.incremental_fns[id.index()] = Some(Box::new(
            move |graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let left_changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<L>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);
                let right_changes = input_changes[1]
                    .as_any()
                    .downcast_ref::<Vec<Change<R>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);

                // Get current states (BEFORE applying changes - states are updated after)
                let left_state = graph
                    .get(left_id)
                    .state
                    .as_any()
                    .downcast_ref::<Multiset<L>>()
                    .cloned()
                    .unwrap_or_default();
                let right_state = graph
                    .get(right_id)
                    .state
                    .as_any()
                    .downcast_ref::<Multiset<R>>()
                    .cloned()
                    .unwrap_or_default();

                let mut output = Vec::new();

                // Process left changes against right state
                if !left_changes.is_empty() {
                    output.extend(operators::join_changes_left(
                        left_changes,
                        &right_state,
                        |l| kl_inc(l),
                        |r| kr_inc(r),
                    ));
                }

                // Process right changes against left state
                if !right_changes.is_empty() {
                    output.extend(operators::join_changes_right(
                        &left_state,
                        right_changes,
                        |l| kl_inc(l),
                        |r| kr_inc(r),
                    ));
                }

                Box::new(output) as Box<dyn AnyChanges>
            },
        ));

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let left_coll = graph
                .get(left_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<L>>()
                .cloned()
                .unwrap_or_default();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<R>>()
                .cloned()
                .unwrap_or_default();
            let output = operators::join(
                &left_coll,
                &right_coll,
                |l| kl_recompute(l),
                |r| kr_recompute(r),
            );
            Box::new(output) as Box<dyn AnyCollection>
        }));

        // Compute initial state
        let left_coll = self
            .graph
            .get(left.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<L>>()
            .cloned()
            .unwrap_or_default();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<R>>()
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
                    Box::new(Multiset::<T>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<T>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the apply function for this node's output type
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<T>());

        // Store the incremental function - union just combines changes
        self.incremental_fns[id.index()] = Some(Box::new(
            move |_graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let left_changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .cloned()
                    .unwrap_or_default();
                let right_changes = input_changes[1]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .cloned()
                    .unwrap_or_default();

                let mut output = left_changes;
                output.extend(right_changes);
                Box::new(output) as Box<dyn AnyChanges>
            },
        ));

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let left_coll = graph
                .get(left_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::union(&left_coll, &right_coll)) as Box<dyn AnyCollection>
        }));

        let left_coll = self
            .graph
            .get(left.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
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
                    Box::new(Multiset::<T>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<T>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the apply function for this node's output type
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<T>());

        // Store the incremental function
        // Distinct needs to track when multiplicities cross the 0 boundary
        self.incremental_fns[id.index()] = Some(Box::new(
            move |graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);

                // Get current input state (before changes are applied)
                let old_input = graph
                    .get(input_id)
                    .state
                    .as_any()
                    .downcast_ref::<Multiset<T>>()
                    .cloned()
                    .unwrap_or_default();

                // Compute new input state by applying changes
                let mut new_input = old_input.clone();
                new_input.apply_changes(changes.iter().cloned());

                // Use the distinct_changes helper
                Box::new(operators::distinct_changes(&old_input, &new_input)) as Box<dyn AnyChanges>
            },
        ));

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::distinct(&input_coll)) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
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
                    Box::new(Multiset::<T>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<T>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the apply function for this node's output type
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<T>());

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let left_coll = graph
                .get(left_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
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
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
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
    ///
    /// Uses BTreeMap internally for O(1) max lookup per group.
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
                    Box::new(Multiset::<(K, V)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(K, V)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function using BTreeMap-based group_max
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            let (_, output) =
                operators::group_max_init(&input_coll, &key_fn_clone, &value_fn_clone);
            Box::new(output) as Box<dyn AnyCollection>
        }));

        // Compute initial state using BTreeMap-based group_max
        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let (_, output) = operators::group_max_init(&input_coll, &key_fn, &value_fn);
        self.graph.get_mut(id).state = Box::new(output);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(K, V)>());

        Relation::new(id)
    }

    /// Group by key and compute minimum value.
    ///
    /// For each distinct key K, outputs (K, min(V)) where V are all values
    /// associated with that key.
    ///
    /// Uses BTreeMap internally for O(1) min lookup per group.
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
                    Box::new(Multiset::<(K, V)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(K, V)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function using BTreeMap-based group_min
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            let (_, output) =
                operators::group_min_init(&input_coll, &key_fn_clone, &value_fn_clone);
            Box::new(output) as Box<dyn AnyCollection>
        }));

        // Compute initial state using BTreeMap-based group_min
        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let (_, output) = operators::group_min_init(&input_coll, &key_fn, &value_fn);
        self.graph.get_mut(id).state = Box::new(output);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(K, V)>());

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
                    Box::new(Multiset::<(K, i64)>::new()) as Box<dyn AnyCollection>,
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
                .downcast_ref::<Multiset<T>>()
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
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::aggregate(&input_coll, &key_fn, &value_fn, |k, vals| {
            operators::sum(k, vals)
        });
        self.graph.get_mut(id).state = Box::new(output);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(K, i64)>());

        Relation::new(id)
    }

    /// Group by key and count tuples.
    ///
    /// For each distinct key K, outputs (K, count) where count is the number
    /// of tuples with that key (weighted by multiplicity).
    pub fn group_count<T, K, FK>(&mut self, input: Relation<T>, key_fn: FK) -> Relation<(K, i64)>
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
                    Box::new(Multiset::<(K, i64)>::new()) as Box<dyn AnyCollection>,
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
                .downcast_ref::<Multiset<T>>()
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
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::aggregate(
            &input_coll,
            &key_fn,
            |_| (),
            |k, vals| operators::count(k, vals),
        );
        self.graph.get_mut(id).state = Box::new(output);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(K, i64)>());

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

    /// Run stratified fixpoint computation for all feedback loops and interrupts.
    ///
    /// Operations are processed in declaration order:
    /// - Feedbacks: run to fixpoint, if any change, restart from the first op
    /// - Interrupts: if relation non-empty, stop immediately
    ///
    /// This continues until a full pass produces no changes, or an interrupt fires.
    fn run_stratified_fixpoint(&mut self) {
        self.interrupted = false;

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

            for i in 0..self.stratified_ops.len() {
                match &self.stratified_ops[i] {
                    StratifiedOp::Interrupt { check } => {
                        // Check if interrupt condition is met
                        if check(&self.graph) {
                            self.interrupted = true;
                            self.graph.clear_dirty();
                            return;
                        }
                    }
                    StratifiedOp::Feedback(_) => {
                        // Process feedback - need to extract data due to borrow checker
                        let (input, var_id) = {
                            let fl = match &self.stratified_ops[i] {
                                StratifiedOp::Feedback(fl) => fl,
                                _ => unreachable!(),
                            };
                            ((fl.compute_input)(&self.graph), fl.var_id)
                        };

                        // Update input_totals and get newly positive tuples
                        // We pass the *next* commit ID (what it will be if there are changes)
                        let next_commit = CommitId::new(self.commit_id.0 + 1);
                        let newly_positive = {
                            let fl = match &mut self.stratified_ops[i] {
                                StratifiedOp::Feedback(fl) => fl,
                                _ => unreachable!(),
                            };
                            fl.ops.add_to_input_totals(
                                fl.input_totals.as_mut(),
                                input.as_ref(),
                                next_commit,
                            )
                        };

                        if !newly_positive.is_empty() {
                            // Actually increment commit ID now that we know there are changes
                            self.commit_id = next_commit;

                            // Record and apply the changes
                            let fl = match &self.stratified_ops[i] {
                                StratifiedOp::Feedback(fl) => fl,
                                _ => unreachable!(),
                            };

                            if self.checkpoint_stack.is_recording() {
                                self.checkpoint_stack.record_feedback_input_deltas(
                                    var_id,
                                    fl.ops.clone_tuples(newly_positive.as_ref()),
                                );
                                self.checkpoint_stack.record_feedback_outputs(
                                    var_id,
                                    fl.ops.clone_tuples(newly_positive.as_ref()),
                                );
                            }

                            fl.ops.apply_output_adds(
                                self.graph.get_mut(var_id).state.as_mut(),
                                newly_positive.as_ref(),
                            );

                            self.recompute_all();
                            iterations += 1;
                            continue 'outer;
                        }
                    }
                }
            }

            break;
        }

        self.graph.clear_dirty();
    }

    fn recompute_all(&mut self) {
        // Sync commit ID to graph so recompute functions can access it
        self.graph.set_commit_id(self.commit_id.0);

        // TODO: Incremental propagation is not yet working correctly with feedback loops.
        // The key issue is that recompute_all is called during feedback fixpoint iteration,
        // and the incremental approach requires all inputs to have been updated first.
        // For now, fall back to full recomputation until we fix the interaction with feedback.
        //
        // if self.propagate_deltas() {
        //     return;
        // }

        // Full recomputation
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

    /// Incrementally propagate deltas through the dataflow graph.
    /// Returns true if successful, false if we should fall back to full recomputation.
    fn propagate_deltas(&mut self) -> bool {
        // Collect pending changes from input nodes
        let topo_order: Vec<NodeId> = self.graph.topo_order().to_vec();

        // Build a map of node_id -> pending changes for this propagation round
        // We'll populate this as we go through the topological order
        let mut pending: std::collections::HashMap<NodeId, Box<dyn AnyChanges>> =
            std::collections::HashMap::new();

        // First, collect pending changes from all input nodes
        for &node_id in &topo_order {
            let node = self.graph.get(node_id);
            if node.is_input() && !node.pending_changes.is_empty() {
                // Take the pending changes - we'll apply them as we propagate
                let changes = self.graph.get_mut(node_id).pending_changes.clone_empty();
                let changes =
                    std::mem::replace(&mut self.graph.get_mut(node_id).pending_changes, changes);
                if !changes.is_empty() {
                    pending.insert(node_id, changes);
                }
            }
        }

        // Now propagate through derived nodes in topological order
        for &node_id in &topo_order {
            let node = self.graph.get(node_id);

            // Skip input and feedback nodes (they have their own update mechanisms)
            if node.is_input() || node.is_feedback() {
                continue;
            }

            // Check if we have an incremental function for this node
            if node_id.index() >= self.incremental_fns.len() {
                continue;
            }

            let incremental_fn = match &self.incremental_fns[node_id.index()] {
                Some(f) => f,
                None => continue, // Fall back to recompute for nodes without incremental
            };

            // Collect input changes for this node
            let inputs = self.graph.get(node_id).inputs.clone();
            let input_changes: Vec<&dyn AnyChanges> = inputs
                .iter()
                .filter_map(|&input_id| pending.get(&input_id).map(|c| c.as_ref()))
                .collect();

            // Skip if no inputs have changes
            if input_changes.is_empty() || input_changes.iter().all(|c| c.is_empty()) {
                continue;
            }

            // Build properly sized input_changes slice
            let empty_changes: Vec<Box<dyn AnyChanges>> = inputs
                .iter()
                .map(|_| Box::new(Vec::<Change<()>>::new()) as Box<dyn AnyChanges>)
                .collect();

            let input_refs: Vec<&dyn AnyChanges> = inputs
                .iter()
                .enumerate()
                .map(|(i, &input_id)| {
                    pending
                        .get(&input_id)
                        .map(|c| c.as_ref())
                        .unwrap_or(empty_changes[i].as_ref())
                })
                .collect();

            // Run the incremental function
            let output_changes = incremental_fn(&self.graph, &input_refs);

            // Store output changes for dependent nodes
            if !output_changes.is_empty() {
                pending.insert(node_id, output_changes);
            }
        }

        // Now apply all changes to update states
        for (node_id, changes) in &pending {
            self.apply_changes_to_node(*node_id, changes.as_ref());
        }

        // Clear all pending changes
        for &node_id in &topo_order {
            self.graph.get_mut(node_id).pending_changes.clear();
        }

        true
    }

    /// Apply type-erased changes to a node's state.
    fn apply_changes_to_node(&mut self, node_id: NodeId, changes: &dyn AnyChanges) {
        // This is a type-erased operation - we need to figure out the type
        // by trying each possible type that's used in the system.
        // This is unfortunate but necessary due to type erasure.

        // The actual application is handled by the state's own methods
        // We'll create a helper trait for this
        let node = self.graph.get_mut(node_id);

        // Use the type info we have from pending_changes to apply correctly
        // For now, we just need to get the changes applied to state
        // The state and changes should be of compatible types

        // Try common types - this is the cost of type erasure
        macro_rules! try_apply {
            ($t:ty) => {
                if let (Some(state), Some(changes)) = (
                    node.state.as_any_mut().downcast_mut::<Multiset<$t>>(),
                    changes.as_any().downcast_ref::<Vec<Change<$t>>>(),
                ) {
                    state.apply_changes(changes.iter().cloned());
                    return;
                }
            };
        }

        // Try common tuple types used in the system
        try_apply!(i32);
        try_apply!((i32, i32));
        try_apply!(((i32, i32), (i32, i32)));
        try_apply!((i32, CommitId));
        try_apply!(((i32, i32), CommitId));
        try_apply!((i32, i64));
        try_apply!(((), i64));
        try_apply!(((), i32));

        // If no type matched, the changes won't be applied
        // This is a limitation we'll need to address
    }

    /// Get an iterator over feedback loops (for pop operations).
    fn feedback_iter(&self) -> impl Iterator<Item = (usize, &FeedbackLoop)> {
        self.stratified_ops
            .iter()
            .enumerate()
            .filter_map(|(i, op)| match op {
                StratifiedOp::Feedback(fl) => Some((i, fl)),
                StratifiedOp::Interrupt { .. } => None,
            })
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

        // Increment commit ID for the global-undo step
        self.next_commit_id();

        // Step 1: Unapply all input changes
        for node_id in frame.changed_input_nodes() {
            if let Some(changes) = frame.get(node_id) {
                changes.unapply(self.graph.get_mut(node_id).state.as_mut());
            }
        }

        // Step 2: For all feedbacks, send -1 for outputs AND subtract recorded input deltas
        // Collect the feedback data we need to process
        let feedback_data: Vec<FeedbackRollbackData> = self
            .feedback_iter()
            .map(|(i, fl)| FeedbackRollbackData {
                index: i,
                var_id: fl.var_id,
                outputs: frame.get_feedback_outputs(fl.var_id).map(|o| o.clone_box()),
                input_deltas: frame
                    .get_feedback_input_deltas(fl.var_id)
                    .map(|d| d.clone_box()),
            })
            .collect();

        // Apply -1 for each output we recorded, and subtract recorded input deltas from input_totals
        for data in &feedback_data {
            if let Some(outputs) = &data.outputs {
                // Need to work around borrow checker by getting ops separately
                let fl = match &self.stratified_ops[data.index] {
                    StratifiedOp::Feedback(fl) => fl,
                    _ => unreachable!(),
                };
                fl.ops.apply_output_removes(
                    self.graph.get_mut(data.var_id).state.as_mut(),
                    outputs.as_ref(),
                );
            }
            if let Some(input_deltas) = &data.input_deltas {
                let fl = match &mut self.stratified_ops[data.index] {
                    StratifiedOp::Feedback(fl) => fl,
                    _ => unreachable!(),
                };
                fl.ops
                    .subtract_from_input_totals(fl.input_totals.as_mut(), input_deltas.as_ref());
            }
        }

        // Step 3: Recompute derived nodes and run fixpoint
        // After subtracting the recorded input deltas, input_totals reflects the pre-push state
        self.recompute_all();

        // Step 4: For each feedback, check if any removed tuples should be re-added
        // (because input_totals is still positive for them after the subtraction)
        for data in &feedback_data {
            if let Some(outputs) = &data.outputs {
                let still_positive = {
                    let fl = match &self.stratified_ops[data.index] {
                        StratifiedOp::Feedback(fl) => fl,
                        _ => unreachable!(),
                    };
                    fl.ops
                        .get_positive_in_totals(fl.input_totals.as_ref(), outputs.as_ref())
                };

                if !still_positive.is_empty() {
                    // Re-add these tuples to output
                    let fl = match &self.stratified_ops[data.index] {
                        StratifiedOp::Feedback(fl) => fl,
                        _ => unreachable!(),
                    };
                    fl.ops.apply_output_adds(
                        self.graph.get_mut(data.var_id).state.as_mut(),
                        still_positive.as_ref(),
                    );

                    // Record in parent frame (if exists)
                    self.checkpoint_stack.record_feedback_outputs_to_parent(
                        data.var_id,
                        fl.ops.clone_tuples(still_positive.as_ref()),
                    );
                }
            }
        }

        // Step 5: Run fixpoint to handle any corrections
        let has_feedbacks = self.feedback_iter().next().is_some();
        if has_feedbacks {
            self.run_partial_stratified_fixpoint();
        }

        true
    }

    /// Run stratified fixpoint (used after pop to re-establish fixpoint).
    /// This is the same as run_stratified_fixpoint but doesn't reset interrupted flag.
    fn run_partial_stratified_fixpoint(&mut self) {
        self.recompute_all();

        let mut iterations = 0;

        'outer: loop {
            if iterations >= self.max_iterations {
                panic!(
                    "Partial stratified fixpoint exceeded max_iterations ({}) - possible infinite loop",
                    self.max_iterations
                );
            }

            for i in 0..self.stratified_ops.len() {
                match &self.stratified_ops[i] {
                    StratifiedOp::Interrupt { check } => {
                        if check(&self.graph) {
                            self.interrupted = true;
                            self.graph.clear_dirty();
                            return;
                        }
                    }
                    StratifiedOp::Feedback(_) => {
                        let (input, var_id) = {
                            let fl = match &self.stratified_ops[i] {
                                StratifiedOp::Feedback(fl) => fl,
                                _ => unreachable!(),
                            };
                            ((fl.compute_input)(&self.graph), fl.var_id)
                        };

                        // Update input_totals and get newly positive tuples
                        // We pass the *next* commit ID (what it will be if there are changes)
                        let next_commit = CommitId::new(self.commit_id.0 + 1);
                        let newly_positive = {
                            let fl = match &mut self.stratified_ops[i] {
                                StratifiedOp::Feedback(fl) => fl,
                                _ => unreachable!(),
                            };
                            fl.ops.add_to_input_totals(
                                fl.input_totals.as_mut(),
                                input.as_ref(),
                                next_commit,
                            )
                        };

                        if !newly_positive.is_empty() {
                            // Actually increment commit ID now that we know there are changes
                            self.commit_id = next_commit;

                            let fl = match &self.stratified_ops[i] {
                                StratifiedOp::Feedback(fl) => fl,
                                _ => unreachable!(),
                            };

                            if self.checkpoint_stack.is_recording() {
                                self.checkpoint_stack.record_feedback_input_deltas(
                                    var_id,
                                    fl.ops.clone_tuples(newly_positive.as_ref()),
                                );
                                self.checkpoint_stack.record_feedback_outputs(
                                    var_id,
                                    fl.ops.clone_tuples(newly_positive.as_ref()),
                                );
                            }

                            fl.ops.apply_output_adds(
                                self.graph.get_mut(var_id).state.as_mut(),
                                newly_positive.as_ref(),
                            );

                            self.recompute_all();
                            iterations += 1;
                            continue 'outer;
                        }
                    }
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
        db.commit();

        let doubled = db.map(nums, |x| x * 2);
        db.commit();

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
        db.commit();

        // Join edges with labels on the source node
        let joined = db.join(edges, labels, |(src, _)| *src, |(id, _)| *id);
        db.commit();

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
        db.commit();

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
        db.insert(numbers, 1);
        db.insert(numbers, 2);
        db.commit();

        let doubled = db.map(numbers, |n| n * 2);
        db.commit();

        // Create checkpoint
        let cp = db.checkpoint(Some("initial"));

        // Verify checkpoint is listed
        let checkpoints = db.list_checkpoints();
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].1, Some("initial"));

        // Make changes
        db.insert(numbers, 3);
        db.delete(numbers, 1);
        db.commit();

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

    #[test]
    fn test_commit_id_and_with_id() {
        let mut db = Database::new();

        // Initial commit ID is 0
        assert_eq!(db.commit_id(), CommitId(0));

        // Create a feedback loop to generate commit ID increments
        let edges = db.create_input::<(i32, i32)>("edges");
        let (path_var, path) = db.variable::<(i32, i32)>("path");

        // Track when each path tuple was discovered
        let paths_with_commit = db.with_id(path);

        // path(a, c) :- path(a, b), edge(b, c)
        let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
        let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
        let all_paths = db.union(edges, new_paths);

        db.feedback(path_var, edges, all_paths);

        // Add edges: 1->2->3
        // This triggers feedback iterations, incrementing commit ID
        db.insert(edges, (1, 2));
        db.insert(edges, (2, 3));
        db.commit();

        // Commit ID should have advanced (once per feedback iteration)
        let commit_after_insert = db.commit_id();
        assert!(
            commit_after_insert > CommitId(0),
            "Commit ID should advance during feedback"
        );

        // Check the paths_with_commit relation
        let paths_with_ids: Vec<_> = db.collect(paths_with_commit);

        // All paths should exist: (1,2), (2,3), (1,3)
        let paths_only: Vec<_> = paths_with_ids.iter().map(|(p, _)| *p).collect();
        assert!(paths_only.contains(&(1, 2)));
        assert!(paths_only.contains(&(2, 3)));
        assert!(paths_only.contains(&(1, 3)));

        // The derived path (1,3) was discovered via feedback
        let derived_id = paths_with_ids
            .iter()
            .find(|(p, _)| *p == (1, 3))
            .map(|(_, id)| *id);

        // All tuples get the same ID in with_id since it's recomputed each time
        // The interesting part is that commit_id advances during feedback
        assert!(derived_id.is_some());

        // Test that pop increments commit ID
        db.push(None);
        db.insert(edges, (3, 4));
        db.commit();
        let commit_before_pop = db.commit_id();

        db.pop();
        let commit_after_pop = db.commit_id();

        assert!(
            commit_after_pop > commit_before_pop,
            "Commit ID should advance on pop: {} vs {}",
            commit_after_pop.raw(),
            commit_before_pop.raw()
        );
    }

    #[test]
    fn test_commit_id_monotonic_through_backtracking() {
        let mut db = Database::new();
        let items = db.create_input::<i32>("items");

        // Track commit IDs through push/pop cycles
        let mut seen_ids = vec![db.commit_id()];

        db.push(None);
        db.insert(items, 1);
        db.commit();
        seen_ids.push(db.commit_id());

        db.push(None);
        db.insert(items, 2);
        db.commit();
        seen_ids.push(db.commit_id());

        // Pop should increment commit ID
        db.pop();
        seen_ids.push(db.commit_id());

        db.pop();
        seen_ids.push(db.commit_id());

        // All commit IDs should be monotonically non-decreasing
        for i in 1..seen_ids.len() {
            assert!(
                seen_ids[i] >= seen_ids[i - 1],
                "Commit IDs should be monotonic: {:?}",
                seen_ids
            );
        }

        // After pops, the ID should have advanced
        assert!(
            seen_ids.last().unwrap() > seen_ids.first().unwrap(),
            "Final commit ID should be greater than initial"
        );
    }

    #[test]
    fn test_commit_id_advances_per_feedback_iteration() {
        // Build a chain: 1 -> 2 -> 3 -> 4 -> 5
        // Each feedback iteration discovers paths one hop longer.
        // We verify the commit ID advances once per iteration by counting
        // how many times it advances for a chain of length N.

        let mut db = Database::new();
        let edges = db.create_input::<(i32, i32)>("edges");

        let initial_commit = db.commit_id();

        // Create the feedback variable for paths
        let (path_var, path) = db.variable::<(i32, i32)>("path");

        // path(a, c) :- path(a, b), edges(b, c)
        let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
        let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
        let all_paths = db.union(edges, new_paths);

        // Wire up the feedback
        db.feedback(path_var, edges, all_paths);

        let after_feedback_setup = db.commit_id();

        // Feedback setup shouldn't advance commit ID (no data yet)
        assert_eq!(
            initial_commit, after_feedback_setup,
            "No commit ID change without data"
        );

        // Add chain edges: 1->2->3->4->5
        // This creates paths of lengths 1, 2, 3, and 4
        // Iteration 1: discover (1,2), (2,3), (3,4), (4,5) - length 1
        // Iteration 2: discover (1,3), (2,4), (3,5) - length 2
        // Iteration 3: discover (1,4), (2,5) - length 3
        // Iteration 4: discover (1,5) - length 4
        // That's 4 feedback iterations = 4 commit ID increments
        db.insert(edges, (1, 2));
        db.insert(edges, (2, 3));
        db.insert(edges, (3, 4));
        db.insert(edges, (4, 5));
        db.commit();

        let after_inserts = db.commit_id();

        // We should have advanced exactly 4 times (once per path length)
        let expected_advances = 4u64;
        let actual_advances = after_inserts.raw() - after_feedback_setup.raw();

        assert_eq!(
            actual_advances, expected_advances,
            "Expected {} commit ID advances for chain of length 4, got {}",
            expected_advances, actual_advances
        );

        // Verify all paths were discovered
        let paths: Vec<_> = db.collect(path);
        assert_eq!(paths.len(), 10); // 4 + 3 + 2 + 1 paths

        // Check specific paths exist
        assert!(paths.contains(&(1, 5)), "Should have path 1->5");
        assert!(paths.contains(&(1, 4)), "Should have path 1->4");
        assert!(paths.contains(&(2, 5)), "Should have path 2->5");
    }

    #[test]
    fn test_feedback_with_id_discovery_order() {
        // Test that feedback_with_id correctly tracks when tuples are discovered.
        // Longer paths should have higher commit IDs than shorter paths.
        let mut db = Database::new();
        let edges = db.create_input::<(i32, i32)>("edges");

        // Add all edges BEFORE setting up feedback, so they're all discovered together
        db.insert(edges, (1, 2));
        db.insert(edges, (2, 3));
        db.insert(edges, (3, 4));
        db.commit();

        // Create a timestamped path variable
        let (path_var, path) = db.variable::<((i32, i32), CommitId)>("path");

        // To build the recursive relation, we need to strip the CommitId,
        // join with edges, then the feedback mechanism re-stamps with new CommitId
        let path_tuples = db.map(path, |((a, b), _)| (*a, *b));

        // path(a, c) :- path(a, b), edges(b, c)
        let extended = db.join(path_tuples, edges, |(_, b)| *b, |(b, _)| *b);
        let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
        let all_paths = db.union(edges, new_paths);

        // Wire up the timestamped feedback - this runs fixpoint and discovers all paths
        db.feedback_with_id(path_var, edges, all_paths);

        // Collect paths with their discovery times
        let paths_with_times: Vec<_> = db.collect(path);

        // Extract commit IDs for paths of different lengths
        let get_commit_id = |from: i32, to: i32| -> Option<CommitId> {
            paths_with_times
                .iter()
                .find(|((a, b), _)| *a == from && *b == to)
                .map(|(_, id)| *id)
        };

        // Length 1 paths: (1,2), (2,3), (3,4)
        let id_1_2 = get_commit_id(1, 2).expect("Should have path 1->2");
        let id_2_3 = get_commit_id(2, 3).expect("Should have path 2->3");
        let id_3_4 = get_commit_id(3, 4).expect("Should have path 3->4");

        // Length 2 paths: (1,3), (2,4)
        let id_1_3 = get_commit_id(1, 3).expect("Should have path 1->3");
        let id_2_4 = get_commit_id(2, 4).expect("Should have path 2->4");

        // Length 3 path: (1,4)
        let id_1_4 = get_commit_id(1, 4).expect("Should have path 1->4");

        // All length-1 paths should have the same commit ID (discovered in same iteration)
        assert_eq!(id_1_2, id_2_3, "Length-1 paths should have same commit ID");
        assert_eq!(id_2_3, id_3_4, "Length-1 paths should have same commit ID");

        // Length-2 paths should have higher commit ID than length-1
        assert!(
            id_1_3 > id_1_2,
            "Length-2 path should be discovered after length-1: {:?} vs {:?}",
            id_1_3,
            id_1_2
        );
        assert_eq!(id_1_3, id_2_4, "Length-2 paths should have same commit ID");

        // Length-3 path should have higher commit ID than length-2
        assert!(
            id_1_4 > id_1_3,
            "Length-3 path should be discovered after length-2: {:?} vs {:?}",
            id_1_4,
            id_1_3
        );
    }
}
