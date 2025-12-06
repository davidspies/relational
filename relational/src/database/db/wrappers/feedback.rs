//! Feedback wrapper for type-erased feedback operations.

/// Type-erased feedback operations.
pub(crate) trait AnyFeedback {
    /// Push a checkpoint.
    fn push_checkpoint(&mut self);
    /// Send -1 for all outputs in the current checkpoint (don't pop yet).
    fn send_inverse(&mut self);
    /// Commit the variable's current changes so they can be pulled by downstream.
    fn commit(&mut self);
    /// Pop checkpoint, pull changes, and forward reachable items.
    fn pop_pull_and_forward(&mut self);
    /// Run one step: pull from input relation, add to variable. Returns true if new output.
    fn step(&mut self) -> bool;
}
