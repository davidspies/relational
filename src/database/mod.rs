//! The main Database type that orchestrates the relational query engine.

mod commit_id;
mod feedback;
mod ops_basic;
mod ops_join;
mod ops_set;
mod aggregation;
mod feedback_setup;
mod fixpoint;
mod propagation;
mod checkpoints_impl;

#[cfg(test)]
mod tests;

use crate::change::{Change, Diff};
use crate::checkpoint::{CheckpointManager, CheckpointStack};
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph, NodeId};
use crate::relation::Relation;
use crate::Tuple;

pub use commit_id::CommitId;
use feedback::StratifiedOp;

/// Type-erased incremental operator function.
type IncrementalFn =
    Box<dyn Fn(&DataflowGraph, &[&dyn AnyChanges]) -> Box<dyn AnyChanges> + Send + Sync>;

/// Type-erased function to apply changes to a node's state.
type ApplyFn = Box<dyn Fn(&mut dyn AnyCollection, &dyn AnyChanges) + Send + Sync>;

/// A type-erased recomputation function that reads input states and produces a new output.
type RecomputeFn = Box<dyn Fn(&DataflowGraph) -> Box<dyn AnyCollection> + Send + Sync>;

/// The main database type managing relations and queries.
pub struct Database {
    /// The dataflow graph of nodes and edges.
    pub(crate) graph: DataflowGraph,
    /// Named checkpoints for state snapshots.
    checkpoints: CheckpointManager,
    /// Stack-based checkpoints for backtracking (push/pop semantics).
    checkpoint_stack: CheckpointStack,
    /// Maximum iterations for fixed-point computation.
    max_iterations: usize,
    /// Type-erased recomputation functions for each derived node.
    recompute_fns: Vec<Option<RecomputeFn>>,
    /// Incremental operator functions for each derived node.
    incremental_fns: Vec<Option<IncrementalFn>>,
    /// Type-erased apply functions for each node.
    apply_fns: Vec<Option<ApplyFn>>,
    /// Feedback loops and interrupts in order of declaration.
    stratified_ops: Vec<StratifiedOp>,
    /// Whether the last fixpoint was interrupted.
    interrupted: bool,
    /// Monotonically increasing commit counter.
    commit_id: CommitId,
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
        self.commit_id.0 += 1;
        self.commit_id
    }

    /// Set the maximum iterations for fixed-point computation.
    pub fn set_max_iterations(&mut self, max: usize) {
        self.max_iterations = max;
    }

    /// Ensure the function vectors are large enough for a node ID.
    fn ensure_recompute_fns_len(&mut self, id: NodeId) {
        let needed = id.index() + 1;
        if self.recompute_fns.len() < needed {
            self.recompute_fns.resize_with(needed, || None);
        }
        if self.incremental_fns.len() < needed {
            self.incremental_fns.resize_with(needed, || None);
        }
        if self.apply_fns.len() < needed {
            self.apply_fns.resize_with(needed, || None);
        }
    }

    /// Create an apply function for a specific tuple type.
    fn make_apply_fn<T: Tuple + Send + Sync>() -> ApplyFn {
        Box::new(|state: &mut dyn AnyCollection, changes: &dyn AnyChanges| {
            if let Some(coll) = state.as_any_mut().downcast_mut::<Multiset<T>>() {
                if let Some(change_vec) = changes.as_any().downcast_ref::<Vec<Change<T>>>() {
                    coll.apply_changes(change_vec.iter().cloned());
                }
            }
        })
    }

    // ========================================================================
    // Input Relations
    // ========================================================================

    /// Create a new input relation.
    pub fn create_input<T: Tuple + Send + Sync>(&mut self, name: &str) -> Relation<T> {
        let id = self.graph.create_input::<T>(name);
        Relation::new(id)
    }

    /// Create a new persistent input relation.
    /// Changes to this relation survive pop().
    pub fn create_persistent_input<T: Tuple + Send + Sync>(&mut self, name: &str) -> Relation<T> {
        let id = self.graph.create_persistent_input::<T>(name);
        Relation::new(id)
    }

    /// Insert a tuple into a relation.
    pub fn insert<T: Tuple + Send + Sync>(&mut self, rel: Relation<T>, tuple: T) {
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
    pub fn delete<T: Tuple + Send + Sync>(&mut self, rel: Relation<T>, tuple: T) {
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
    pub fn commit(&mut self) {
        if self.stratified_ops.is_empty() {
            self.recompute_all();
            self.graph.clear_dirty();
        } else {
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
            .unwrap_or(Diff(0))
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}
