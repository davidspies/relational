//! The main Database type that orchestrates the relational query engine.

mod aggregation;
mod aggregation_sum;
mod checkpoints_named;
mod checkpoints_stack;
mod commit_id;
mod feedback;
mod feedback_ops;
mod feedback_ops_timestamped;
mod feedback_setup;
mod fixpoint;
mod input;
mod ops_difference;
mod ops_distinct;
mod ops_filter;
mod ops_flat_map;
mod ops_join;
mod ops_map;
mod ops_union;
mod propagation;
mod query;

#[cfg(test)]
mod tests;

use crate::Tuple;
use crate::change::Change;
use crate::checkpoint::{CheckpointManager, CheckpointStack};
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph, NodeId};

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
            if let Some(coll) = state.as_any_mut().downcast_mut::<Multiset<T>>()
                && let Some(change_vec) = changes.as_any().downcast_ref::<Vec<Change<T>>>()
            {
                coll.apply_changes(change_vec.iter().cloned());
            }
        })
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}
