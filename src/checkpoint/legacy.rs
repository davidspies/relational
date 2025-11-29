//! Legacy checkpoint types for backwards compatibility.

use std::collections::HashMap;

use crate::dataflow::NodeId;

/// A checkpoint identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckpointId(pub(crate) usize);

impl CheckpointId {
    pub fn index(&self) -> usize {
        self.0
    }
}

/// A named checkpoint storing state snapshots.
pub struct Checkpoint {
    pub id: CheckpointId,
    pub name: Option<String>,
    pub(crate) states: HashMap<NodeId, Box<dyn crate::dataflow::AnyCollection>>,
    pub(crate) manual_inputs: Vec<NodeId>,
}

impl Clone for Checkpoint {
    fn clone(&self) -> Self {
        Checkpoint {
            id: self.id,
            name: self.name.clone(),
            states: self
                .states
                .iter()
                .map(|(k, v)| (*k, v.clone_box()))
                .collect(),
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

    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("checkpoint_{}", self.id.0))
    }
}

/// Manager for named checkpoints.
pub struct CheckpointManager {
    checkpoints: Vec<Checkpoint>,
    next_id: usize,
}

impl CheckpointManager {
    pub fn new() -> Self {
        CheckpointManager {
            checkpoints: Vec::new(),
            next_id: 0,
        }
    }

    pub fn next_id(&mut self) -> CheckpointId {
        let id = CheckpointId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn store(&mut self, checkpoint: Checkpoint) {
        if let Some(pos) = self.checkpoints.iter().position(|c| c.id == checkpoint.id) {
            self.checkpoints[pos] = checkpoint;
        } else {
            self.checkpoints.push(checkpoint);
        }
    }

    pub fn get(&self, id: CheckpointId) -> Option<&Checkpoint> {
        self.checkpoints.iter().find(|c| c.id == id)
    }

    pub fn list(&self) -> &[Checkpoint] {
        &self.checkpoints
    }
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a restore operation.
#[derive(Debug, Default)]
pub struct RestoreInfo {
    pub restored_nodes: Vec<NodeId>,
    pub manual_input_nodes: Vec<NodeId>,
    pub needs_propagation: bool,
}

impl RestoreInfo {
    pub fn new() -> Self {
        Self::default()
    }
}
