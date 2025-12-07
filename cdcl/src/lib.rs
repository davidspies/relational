//! CDCL SAT Solver built on the relational query engine.
//!
//! This implements Conflict-Driven Clause Learning using:
//! - Relations for clauses, assignments, and implications
//! - Feedback loops for unit propagation (fixpoint)
//! - Push/pop checkpoints for backtracking
//! - Persistent inputs for learned clauses

mod clause_deletion;
mod cnf;
mod conflict_analysis;
mod conflicts_sink;
mod queries;
mod restart;
mod solve;
mod solver;
mod solver_setup;
mod types;
mod vsids;

pub mod proof;

#[cfg(test)]
mod tests;

pub use cnf::Cnf;
pub use solver::Solver;
pub use types::{Lit, Var};
