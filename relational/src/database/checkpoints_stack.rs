//! Stack-based push/pop checkpoint operations for Database.

use super::feedback::{FeedbackLoop, FeedbackRollbackData, StratifiedOp};
use super::Database;

impl Database {
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

        // Apply -1 for each output we recorded, and subtract recorded input deltas
        for data in &feedback_data {
            if let Some(outputs) = &data.outputs {
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

        // Step 4: For each feedback, check if removed tuples should be re-added
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
                    let fl = match &self.stratified_ops[data.index] {
                        StratifiedOp::Feedback(fl) => fl,
                        _ => unreachable!(),
                    };
                    fl.ops.apply_output_adds(
                        self.graph.get_mut(data.var_id).state.as_mut(),
                        still_positive.as_ref(),
                    );

                    fl.ops.append_insert_changes(
                        self.graph.get_mut(data.var_id).pending_changes.as_mut(),
                        still_positive.as_ref(),
                    );
                    self.graph.mark_dirty(data.var_id);

                    self.checkpoint_stack.record_feedback_outputs_to_parent(
                        data.var_id,
                        fl.ops.clone_tuples(still_positive.as_ref()),
                    );
                }
            }

            self.run_partial_stratified_fixpoint(data.index);
        }

        true
    }

    /// Check if we're currently recording changes.
    pub fn is_recording(&self) -> bool {
        self.checkpoint_stack.is_recording()
    }

    /// Get the current checkpoint stack depth.
    pub fn stack_depth(&self) -> usize {
        self.checkpoint_stack.depth()
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
