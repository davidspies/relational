//! Pull-based differential dataflow with feedback support.
//!
//! This module is organized in layers:
//! - `relational`: Pull-based relational operators (map, filter, join, etc.)
//! - `feedback`: Push-based feedback/fixpoint primitives
//! - `db`: Central Database for coordinating commit, push/pop, and fixpoint

mod commit_id;
mod db;
pub(crate) mod feedback;
pub(crate) mod relational;

#[cfg(test)]
mod tests;

// Re-export relational types at top level for convenience
pub use relational::*;

// Re-export database
pub use commit_id::CommitId;
pub use db::{Database, DatabaseBuilder};
