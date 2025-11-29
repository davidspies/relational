//! Pull-based differential dataflow with feedback support.
//!
//! This module is organized in two layers:
//! - `relational`: Pull-based relational operators (map, filter, join, etc.)
//! - `feedback`: Push-based feedback/fixpoint system built on top

pub mod feedback;
pub mod relational;

#[cfg(test)]
mod tests;

// Re-export relational types at top level for convenience
pub use relational::*;

// Re-export feedback types
pub use feedback::{fixpoint, Iteration, Variable};
