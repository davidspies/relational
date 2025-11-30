//! Checkpoint system for backtracking state.
//!
//! Uses a stack-based approach with speculative execution. On pop:
//! 1. Simultaneously undo all recorded changes to inputs + feedbacks
//! 2. Resolve corrections in stratified order as recomputation reveals
//!    actual vs expected differences

mod changes_trait;
mod frame;
mod legacy;
mod stack;

pub use changes_trait::AnyChanges;
pub use frame::CheckpointFrame;
pub use legacy::{Checkpoint, CheckpointId, CheckpointManager, RestoreInfo};
pub use stack::CheckpointStack;
