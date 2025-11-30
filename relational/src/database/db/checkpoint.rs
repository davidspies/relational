//! Checkpoint management for Database - push/pop operations.

use super::{Database, StratifiedStep};

impl Database {
    /// Push a new checkpoint level.
    pub fn push(&mut self) {
        self.checkpoint_depth += 1;
        for input in &mut self.inputs {
            input.push_checkpoint();
        }
        for step in &mut self.steps {
            if let StratifiedStep::Feedback(feedback) = step {
                feedback.push_checkpoint();
            }
        }
    }

    /// Pop a checkpoint level, undoing all changes since the matching push.
    ///
    /// Algorithm:
    /// 1. Unapply non-persistent input AND feedback changes by sending -1's
    ///    - For non-persistent inputs, also pop the checkpoint from the stack
    ///    - Don't pop for feedbacks yet
    /// 2. Commit changes
    /// 3. In stratified order of feedbacks, for each feedback:
    ///    a) Pull all changes and update tracked inputs; forward along anything
    ///    which is NOT in the last checkpoint with a +1
    ///    b) Pop the last checkpoint; forward along a +1 for anything in that
    ///    checkpoint whose tracked input value is still non-zero
    ///    c) Propagate normally all feedbacks up to and including this one
    ///    (using nested loop approach where you restart from beginning if changes)
    #[must_use]
    pub fn pop(&mut self) -> bool {
        if self.checkpoint_depth == 0 {
            return false;
        }

        self.checkpoint_depth -= 1;

        // Step 1: Unapply non-persistent input AND feedback changes by sending -1's
        // For non-persistent inputs, also pop the checkpoint from the stack
        for input in &mut self.inputs {
            input.send_inverse_and_pop();
        }
        // For feedbacks, send -1's but don't pop yet
        for step in &mut self.steps {
            if let StratifiedStep::Feedback(feedback) = step {
                feedback.send_inverse();
                // Step 2: Commit feedback changes so they can be pulled
                feedback.commit();
            }
        }

        // Increment commit ID after making changes dirty
        self.increment_commit_id();

        // Step 3: In stratified order, for each feedback:
        //   - pull_and_forward_non_checkpoint, pop_and_forward_reachable, commit
        //   - Then run fixpoint up to this step
        // If an interrupt fires, continue processing feedbacks

        for i in 0..self.steps.len() {
            if let StratifiedStep::Feedback(feedback) = &mut self.steps[i] {
                // 3a) Pull all changes and update tracked inputs; forward along anything
                //     which is NOT in the last checkpoint with a +1
                feedback.pull_and_forward_non_checkpoint();

                // 3b) Pop the last checkpoint; forward along a +1 for anything in that
                //     checkpoint whose tracked input value is still non-zero
                feedback.pop_and_forward_reachable();

                // Commit the forwarded changes so they can be pulled
                feedback.commit();

                // Increment commit ID so SavedRelations see the forwarded changes
                self.increment_commit_id();

                // 3c) Propagate normally all steps up to and including this one
                self.run_stratified_fixpoint_up_to(i);
            }
        }

        // Reset all interrupts so they don't keep firing on subsequent commits
        for step in &mut self.steps {
            if let StratifiedStep::Interrupt(interrupt) = step {
                interrupt.reset();
            }
        }

        true
    }

    /// Get the current checkpoint depth.
    pub fn depth(&self) -> usize {
        self.checkpoint_depth
    }
}
