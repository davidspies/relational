//! CDCL SAT Solver built on the relational query engine.
//!
//! This implements Conflict-Driven Clause Learning using:
//! - Relations for clauses, assignments, and implications
//! - Feedback loops for unit propagation (fixpoint)
//! - Push/pop checkpoints for backtracking
//! - Persistent inputs for learned clauses

mod assignments_sink;
mod cause_sink;
mod conflict_analysis;
mod queries;
mod solve;
mod solver;
mod solver_setup;
mod types;

#[cfg(test)]
mod tests;

pub use solver::Solver;
pub use types::{ClauseId, Conflict, Level, Lit, Var};
