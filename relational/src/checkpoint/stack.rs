//! Stack-based checkpoint manager for efficient backtracking.

use crate::change::Change;
use crate::dataflow::NodeId;
use crate::Tuple;

use super::CheckpointFrame;

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
