//! Push-based feedback and fixpoint computation.
//!
//! This layer sits on top of the pull-based relational operators to enable
//! recursive computations like transitive closure.
//!
//! The key abstraction is the `Variable` which:
//! 1. Tracks a "seen set" - tuples are only emitted once when they first become positive
//! 2. Supports checkpointing for push/pop semantics
//! 3. Tracks input_counts for reachability during pop

mod variable;

pub use variable::Variable;
