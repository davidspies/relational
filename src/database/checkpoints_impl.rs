//! Checkpoint and push/pop operations for Database.

use crate::checkpoint::{Checkpoint, CheckpointId, RestoreInfo};
use crate::dataflow::NodeId;
use crate::relation::Relation;
use crate::Tuple;

use super::feedback::{FeedbackLoop, FeedbackRollbackData, StratifiedOp};
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

        // Step 3: Recompute derived nodes
        self.recompute_all();

        // Step 4: For each feedback in stratified order, check if any removed tuples
        // should be re-added, then run partial fixpoint up to that feedback
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

                    // Add pending changes for incremental propagation
                    fl.ops.append_insert_changes(
                        self.graph.get_mut(data.var_id).pending_changes.as_mut(),
                        still_positive.as_ref(),
                    );
                    self.graph.mark_dirty(data.var_id);

                    // Record in parent frame (if exists)
                    self.checkpoint_stack.record_feedback_outputs_to_parent(
                        data.var_id,
                        fl.ops.clone_tuples(still_positive.as_ref()),
                    );
                }
            }

            // Run partial fixpoint up to this feedback index
            self.run_partial_stratified_fixpoint(data.index);
        }

        true
    }

    /// Check if we're currently recording changes (have at least one frame on the stack).
    pub fn is_recording(&self) -> bool {
        self.checkpoint_stack.is_recording()
    }

    /// Get the current checkpoint stack depth.
    pub fn stack_depth(&self) -> usize {
        self.checkpoint_stack.depth()
    }

    /// Get a relation by name.
    pub fn get<T: Tuple>(&self, name: &str) -> Option<Relation<T>> {
        self.graph.get_id(name).map(Relation::new)
    }

    /// Get an iterator over feedback loops (for pop operations).
    pub(super) fn feedback_iter(&self) -> impl Iterator<Item = (usize, &FeedbackLoop)> {
        self.stratified_ops
            .iter()
            .enumerate()
            .filter_map(|(i, op)| match op {
                StratifiedOp::Feedback(fl) => Some((i, fl)),
                StratifiedOp::Interrupt { .. } => None,
            })
    }
}
