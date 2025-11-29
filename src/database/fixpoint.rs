//! Fixed-point computation for Database.

use super::commit_id::CommitId;
use super::feedback::StratifiedOp;
use super::Database;

impl Database {
    /// Run stratified fixpoint computation for all feedback loops and interrupts.
    pub(super) fn run_stratified_fixpoint(&mut self) {
        self.interrupted = false;
        self.run_fixpoint_up_to(self.stratified_ops.len());
    }

    /// Run stratified fixpoint for feedbacks 0..=up_to_index only.
    /// Used during pop() to incrementally re-establish fixpoint.
    pub(super) fn run_partial_stratified_fixpoint(&mut self, up_to_index: usize) {
        self.run_fixpoint_up_to(up_to_index + 1);
    }

    /// Run fixpoint computation for stratified_ops[0..limit].
    fn run_fixpoint_up_to(&mut self, limit: usize) {
        self.recompute_all();

        let mut iterations = 0;

        'outer: loop {
            if iterations >= self.max_iterations {
                panic!(
                    "Stratified fixpoint exceeded max_iterations ({}) - possible infinite loop",
                    self.max_iterations
                );
            }

            for i in 0..limit.min(self.stratified_ops.len()) {
                match &self.stratified_ops[i] {
                    StratifiedOp::Interrupt { check } => {
                        if check(&self.graph) {
                            self.interrupted = true;
                            self.graph.clear_dirty();
                            return;
                        }
                    }
                    StratifiedOp::Feedback(_) => {
                        let (input, var_id) = {
                            let fl = match &self.stratified_ops[i] {
                                StratifiedOp::Feedback(fl) => fl,
                                _ => unreachable!(),
                            };
                            ((fl.compute_input)(&self.graph), fl.var_id)
                        };

                        // Update input_totals and get newly positive tuples
                        let next_commit = CommitId::new(self.commit_id.0 + 1);
                        let newly_positive = {
                            let fl = match &mut self.stratified_ops[i] {
                                StratifiedOp::Feedback(fl) => fl,
                                _ => unreachable!(),
                            };
                            fl.ops.add_to_input_totals(
                                fl.input_totals.as_mut(),
                                input.as_ref(),
                                next_commit,
                            )
                        };

                        if !newly_positive.is_empty() {
                            self.commit_id = next_commit;

                            let fl = match &self.stratified_ops[i] {
                                StratifiedOp::Feedback(fl) => fl,
                                _ => unreachable!(),
                            };

                            if self.checkpoint_stack.is_recording() {
                                self.checkpoint_stack.record_feedback_input_deltas(
                                    var_id,
                                    fl.ops.clone_tuples(newly_positive.as_ref()),
                                );
                                self.checkpoint_stack.record_feedback_outputs(
                                    var_id,
                                    fl.ops.clone_tuples(newly_positive.as_ref()),
                                );
                            }

                            fl.ops.apply_output_adds(
                                self.graph.get_mut(var_id).state.as_mut(),
                                newly_positive.as_ref(),
                            );

                            fl.ops.append_insert_changes(
                                self.graph.get_mut(var_id).pending_changes.as_mut(),
                                newly_positive.as_ref(),
                            );
                            self.graph.mark_dirty(var_id);

                            self.recompute_all();
                            iterations += 1;
                            continue 'outer;
                        }
                    }
                }
            }

            break;
        }

        self.graph.clear_dirty();
    }
}
