//! Checkpoint system for backtracking state.
//!
//! Checkpoints save the state of derived relations, allowing you to
//! backtrack after making changes. Manual inputs are NOT automatically
//! reverted - users decide what to keep/change.

use std::collections::HashMap;

use crate::dataflow::{AnyCollection, NodeId};

/// A checkpoint capturing the state of the database at a point in time.
pub struct Checkpoint {
    /// Unique identifier for this checkpoint.
    pub id: CheckpointId,
    /// Optional name for the checkpoint.
    pub name: Option<String>,
    /// Saved state for each node (by node ID).
    pub(crate) states: HashMap<NodeId, Box<dyn AnyCollection>>,
    /// Which nodes are manual inputs (won't be auto-reverted).
    pub(crate) manual_inputs: Vec<NodeId>,
}

impl Clone for Checkpoint {
    fn clone(&self) -> Self {
        Checkpoint {
            id: self.id,
            name: self.name.clone(),
            states: self.states.iter().map(|(k, v)| (*k, v.clone_box())).collect(),
            manual_inputs: self.manual_inputs.clone(),
        }
    }
}

impl Checkpoint {
    pub fn new(id: CheckpointId, name: Option<String>) -> Self {
        Checkpoint {
            id,
            name,
            states: HashMap::new(),
            manual_inputs: Vec::new(),
        }
    }

    /// Get the checkpoint name or a default.
    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| format!("checkpoint_{}", self.id.0))
    }
}

/// A unique identifier for a checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckpointId(pub(crate) usize);

impl CheckpointId {
    pub fn index(&self) -> usize {
        self.0
    }
}

/// Manages checkpoints for a database.
pub struct CheckpointManager {
    /// All checkpoints, indexed by ID.
    checkpoints: Vec<Checkpoint>,
    /// Current checkpoint counter.
    next_id: usize,
}

impl CheckpointManager {
    pub fn new() -> Self {
        CheckpointManager {
            checkpoints: Vec::new(),
            next_id: 0,
        }
    }

    /// Create a new checkpoint ID.
    pub fn next_id(&mut self) -> CheckpointId {
        let id = CheckpointId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Store a checkpoint.
    pub fn store(&mut self, checkpoint: Checkpoint) {
        // Find existing or push new
        if let Some(pos) = self.checkpoints.iter().position(|c| c.id == checkpoint.id) {
            self.checkpoints[pos] = checkpoint;
        } else {
            self.checkpoints.push(checkpoint);
        }
    }

    /// Get a checkpoint by ID.
    pub fn get(&self, id: CheckpointId) -> Option<&Checkpoint> {
        self.checkpoints.iter().find(|c| c.id == id)
    }

    /// List all checkpoints.
    pub fn list(&self) -> &[Checkpoint] {
        &self.checkpoints
    }

}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about what changed when restoring a checkpoint.
#[derive(Debug, Default)]
pub struct RestoreInfo {
    /// Nodes that were restored to checkpoint state.
    pub restored_nodes: Vec<NodeId>,
    /// Manual input nodes that were NOT restored (user must decide).
    pub manual_input_nodes: Vec<NodeId>,
    /// Whether any re-propagation is needed.
    pub needs_propagation: bool,
}

impl RestoreInfo {
    pub fn new() -> Self {
        Self::default()
    }
}
