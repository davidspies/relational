//! CDCL SAT Solver built on the relational query engine.
//!
//! This implements Conflict-Driven Clause Learning using:
//! - Relations for clauses, assignments, and implications
//! - Feedback loops for unit propagation (fixpoint)
//! - Push/pop checkpoints for backtracking
//! - Persistent inputs for learned clauses

mod types;
mod solver;
mod queries;
mod solve;

#[cfg(test)]
mod tests;

pub use types::{ClauseId, Conflict, Level, Lit, Var};
pub use solver::Solver;
