//! Traits for checkpoint/restore operations.
//!
//! The database pop() algorithm:
//! 1. Send -1's for all input and feedback changes, pop input checkpoints
//! 2. Commit inputs
//! 3. For each feedback in stratified order:
//!    a) Pull changes, update tracked inputs, forward +1 for non-checkpoint items
//!    b) Pop checkpoint, forward +1 for reachable items in checkpoint
//!    c) Run fixpoint for feedbacks 0..=i

// The actual trait definitions are in db/wrappers.rs since they need
// access to the concrete types. This module is kept for documentation.
