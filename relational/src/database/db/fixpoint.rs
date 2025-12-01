//! Fixpoint computation for Database.

use super::{Database, StratifiedStep};

impl Database {
    /// Commit staged changes and run stratified fixpoint.
    pub fn commit(&mut self) {
        // Record any pending inserts to the current checkpoint level
        for input in &mut self.inputs {
            input.record_pending_inserts();
        }

        // Increment commit ID after making changes dirty
        self.increment_commit_id();

        // Then run stratified fixpoint (feedbacks commit in step())
        self.run_stratified_fixpoint();
    }

    /// Run stratified fixpoint computation for all steps (feedbacks and interrupts).
    pub(super) fn run_stratified_fixpoint(&mut self) {
        if !self.steps.is_empty() {
            self.run_stratified_fixpoint_up_to(self.steps.len() - 1);
        }
    }

    /// Run stratified fixpoint up to and including the given step index.
    pub(super) fn run_stratified_fixpoint_up_to(&mut self, limit: usize) {
        let recording = self.checkpoint_depth > 0;
        let mut iterations = 0;

        'outer: loop {
            if iterations >= self.max_iterations {
                panic!(
                    "Stratified fixpoint exceeded max_iterations ({}) - possible infinite loop",
                    self.max_iterations
                );
            }

            for i in 0..=limit {
                match &mut self.steps[i] {
                    StratifiedStep::Interrupt(interrupt) => {
                        if interrupt.check() {
                            return;
                        }
                    }
                    StratifiedStep::Feedback(feedback) => {
                        if feedback.step(recording) {
                            // Increment commit ID so SavedRelations know to re-pull
                            self.increment_commit_id();
                            iterations += 1;
                            continue 'outer;
                        }
                    }
                }
            }

            // No feedback produced new output, we're done
            break;
        }
    }
}
