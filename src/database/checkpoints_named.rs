//! Named checkpoint operations for Database.

use crate::checkpoint::{Checkpoint, CheckpointId, RestoreInfo};
use crate::dataflow::NodeId;
use crate::relation::Relation;
use crate::Tuple;

use super::Database;

impl Database {
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

        let mut to_restore: Vec<(NodeId, Box<dyn crate::dataflow::AnyCollection>)> = Vec::new();

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

    /// Get a relation by name.
    pub fn get<T: Tuple>(&self, name: &str) -> Option<Relation<T>> {
        self.graph.get_id(name).map(Relation::new)
    }
}
