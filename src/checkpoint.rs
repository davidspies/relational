//! Checkpoint system for backtracking state.
//!
//! Uses a stack-based approach with speculative execution. On pop:
//! 1. Simultaneously undo all recorded changes to inputs + feedbacks
//! 2. Resolve corrections in stratified order as recomputation reveals
//!    actual vs expected differences

use std::collections::HashMap;

use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::NodeId;
use crate::Tuple;

/// Type-erased changes that can be manipulated.
pub trait AnyChanges: Send + Sync {
    /// Unapply these changes (negate and apply).
    fn unapply(&self, state: &mut dyn crate::dataflow::AnyCollection);
    /// Clone into a box.
    fn clone_box(&self) -> Box<dyn AnyChanges>;
    /// Downcast to mutable Any for type checking.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl<T: Tuple + Send + Sync> AnyChanges for Vec<Change<T>> {
    fn unapply(&self, state: &mut dyn crate::dataflow::AnyCollection) {
        if let Some(coll) = state.as_any_mut().downcast_mut::<Multiset<T>>() {
            let negated: Vec<Change<T>> = self
                .iter()
                .map(|c| Change {
                    tuple: c.tuple.clone(),
                    diff: -c.diff,
                })
                .collect();
            coll.apply_changes(negated);
        }
    }

    fn clone_box(&self) -> Box<dyn AnyChanges> {
        Box::new(self.clone())
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

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
    pub fn get_feedback_input_deltas(&self, var_id: NodeId) -> Option<&dyn crate::dataflow::AnyCollection> {
        self.feedback_input_deltas.get(&var_id).map(|b| b.as_ref())
    }

    /// Get the recorded input changes for a node.
    pub fn get(&self, node_id: NodeId) -> Option<&dyn AnyChanges> {
        self.input_changes.get(&node_id).map(|b| b.as_ref())
    }

    /// Get the recorded feedback outputs for a node.
    pub fn get_feedback_outputs(&self, var_id: NodeId) -> Option<&dyn crate::dataflow::AnyCollection> {
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

/// A stack-based checkpoint manager for efficient backtracking.
pub struct CheckpointStack {
    frames: Vec<CheckpointFrame>,
}

impl CheckpointStack {
    pub fn new() -> Self {
        CheckpointStack { frames: Vec::new() }
    }

    /// Push a new checkpoint frame onto the stack.
    pub fn push(&mut self, name: Option<String>) -> usize {
        self.frames.push(CheckpointFrame::new(name));
        self.frames.len()
    }

    /// Pop the top checkpoint frame.
    pub fn pop(&mut self) -> Option<CheckpointFrame> {
        self.frames.pop()
    }

    /// Record changes to the current frame (top of stack).
    pub fn record<T: Tuple + Send + Sync>(&mut self, node_id: NodeId, changes: Vec<Change<T>>) {
        if let Some(frame) = self.frames.last_mut() {
            frame.record(node_id, changes);
        }
    }

    /// Record feedback output additions to the current frame.
    pub fn record_feedback_outputs(
        &mut self,
        var_id: NodeId,
        tuples: Box<dyn crate::dataflow::AnyCollection>,
    ) {
        if let Some(frame) = self.frames.last_mut() {
            frame.record_feedback_outputs(var_id, tuples);
        }
    }

    /// Record feedback output additions to the previous frame (for corrections during pop).
    pub fn record_feedback_outputs_to_parent(
        &mut self,
        var_id: NodeId,
        tuples: Box<dyn crate::dataflow::AnyCollection>,
    ) {
        if let Some(frame) = self.frames.last_mut() {
            frame.record_feedback_outputs(var_id, tuples);
        }
    }

    /// Record feedback input deltas to the current frame.
    pub fn record_feedback_input_deltas(
        &mut self,
        var_id: NodeId,
        deltas: Box<dyn crate::dataflow::AnyCollection>,
    ) {
        if let Some(frame) = self.frames.last_mut() {
            frame.record_feedback_input_deltas(var_id, deltas);
        }
    }

    /// Get the current stack depth.
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Check if we're currently recording (have at least one frame).
    pub fn is_recording(&self) -> bool {
        !self.frames.is_empty()
    }
}

impl Default for CheckpointStack {
    fn default() -> Self {
        Self::new()
    }
}

// Legacy types for backwards compatibility

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckpointId(pub(crate) usize);

impl CheckpointId {
    pub fn index(&self) -> usize {
        self.0
    }
}

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
