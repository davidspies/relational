//! Pull-based differential dataflow with feedback support.
//!
//! This module is organized in layers:
//! - `relational`: Pull-based relational operators (map, filter, join, etc.)
//! - `feedback`: Push-based feedback/fixpoint primitives
//! - `db`: Central Database2 for coordinating commit, push/pop, and fixpoint

mod commit_id;
mod db;
pub mod feedback;
pub mod relational;

#[cfg(test)]
mod tests;

// Re-export relational types at top level for convenience
pub use relational::*;

// Re-export feedback types
pub use feedback::Variable;

// Re-export database
pub use commit_id::CommitId;
pub use db::Database2;
