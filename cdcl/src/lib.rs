//! CDCL SAT Solver built on the relational query engine.
//!
//! This implements Conflict-Driven Clause Learning using:
//! - Relations for clauses, assignments, and implications
//! - Feedback loops for unit propagation (fixpoint)
//! - Push/pop checkpoints for backtracking
//! - Persistent inputs for learned clauses

mod cause_sink;
mod clause_deletion;
pub mod cnf;
mod conflict_analysis;
mod conflicts_sink;
pub mod proof;
mod queries;
mod restart;
mod solve;
mod solver;
mod solver_setup;
mod types;
mod vsids;

#[cfg(test)]
mod tests;

pub use cnf::{Cnf, SolveResult};
pub use solver::Solver;
pub use types::{ClauseId, Conflict, Level, Lit, Var};
