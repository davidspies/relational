//! Checkpoint frame for storing changes since the previous frame.

use std::collections::HashMap;

use crate::change::Change;
use crate::dataflow::NodeId;
use crate::Tuple;

use super::AnyChanges;

/// A frame on the checkpoint stack, storing changes since the previous frame.
pub struct CheckpointFrame {
    /// Optional name for this checkpoint.
    pub name: Option<String>,
    /// Changes made to input nodes since the previous checkpoint.
    /// These are the changes TO UNDO when popping.
    input_changes: HashMap<NodeId, Box<dyn AnyChanges>>,
    /// Feedback output additions (tuples we sent +1 for).
    /// Key is feedback var_id, value is a Collection of tuples added.
    feedback_outputs: HashMap<NodeId, Box<dyn crate::dataflow::AnyCollection>>,
    /// Feedback input deltas (what we added to input_totals during this checkpoint).
    /// Key is feedback var_id, value is a Collection tracking multiplicity deltas.
    feedback_input_deltas: HashMap<NodeId, Box<dyn crate::dataflow::AnyCollection>>,
}

impl CheckpointFrame {
    pub fn new(name: Option<String>) -> Self {
        CheckpointFrame {
            name,
            input_changes: HashMap::new(),
            feedback_outputs: HashMap::new(),
            feedback_input_deltas: HashMap::new(),
        }
    }

    /// Record changes for an input node (accumulates).
    pub fn record<T: Tuple + Send + Sync>(&mut self, node_id: NodeId, new_changes: Vec<Change<T>>) {
        if new_changes.is_empty() {
            return;
        }

        if let Some(existing) = self.input_changes.get_mut(&node_id) {
            // Try to merge - apply the new changes to existing
            if let Some(existing_vec) = existing.as_any_mut().downcast_mut::<Vec<Change<T>>>() {
                existing_vec.extend(new_changes);
            }
        } else {
            self.input_changes.insert(node_id, Box::new(new_changes));
        }
    }

    /// Record feedback output additions (tuples we sent +1 for).
    pub fn record_feedback_outputs(
        &mut self,
        var_id: NodeId,
        tuples: Box<dyn crate::dataflow::AnyCollection>,
    ) {
        if let Some(existing) = self.feedback_outputs.get_mut(&var_id) {
            // Merge by adding all tuples from the new collection
            existing.merge_from(tuples.as_ref());
        } else {
            self.feedback_outputs.insert(var_id, tuples);
        }
    }

    /// Record feedback input deltas (what we added to input_totals).
    pub fn record_feedback_input_deltas(
        &mut self,
        var_id: NodeId,
        deltas: Box<dyn crate::dataflow::AnyCollection>,
    ) {
        if let Some(existing) = self.feedback_input_deltas.get_mut(&var_id) {
            existing.merge_from(deltas.as_ref());
        } else {
            self.feedback_input_deltas.insert(var_id, deltas);
        }
    }

    /// Get the recorded feedback input deltas for a node.
    pub fn get_feedback_input_deltas(
        &self,
        var_id: NodeId,
    ) -> Option<&dyn crate::dataflow::AnyCollection> {
        self.feedback_input_deltas.get(&var_id).map(|b| b.as_ref())
    }

    /// Get the recorded input changes for a node.
    pub fn get(&self, node_id: NodeId) -> Option<&dyn AnyChanges> {
        self.input_changes.get(&node_id).map(|b| b.as_ref())
    }

    /// Get the recorded feedback outputs for a node.
    pub fn get_feedback_outputs(
        &self,
        var_id: NodeId,
    ) -> Option<&dyn crate::dataflow::AnyCollection> {
        self.feedback_outputs.get(&var_id).map(|b| b.as_ref())
    }

    /// Get all node IDs that have input changes.
    pub fn changed_input_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.input_changes.keys().copied()
    }

    /// Check if this frame has any changes.
    pub fn has_changes(&self) -> bool {
        !self.input_changes.is_empty() || !self.feedback_outputs.is_empty()
    }
}

impl Clone for CheckpointFrame {
    fn clone(&self) -> Self {
        CheckpointFrame {
            name: self.name.clone(),
            input_changes: self
                .input_changes
                .iter()
                .map(|(k, v)| (*k, v.clone_box()))
                .collect(),
            feedback_outputs: self
                .feedback_outputs
                .iter()
                .map(|(k, v)| (*k, v.clone_box()))
                .collect(),
            feedback_input_deltas: self
                .feedback_input_deltas
                .iter()
                .map(|(k, v)| (*k, v.clone_box()))
                .collect(),
        }
    }
}
