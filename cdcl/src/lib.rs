//! CDCL SAT Solver built on the relational query engine.
//!
//! This implements Conflict-Driven Clause Learning using:
//! - Relations for clauses, assignments, and implications
//! - Feedback loops for unit propagation (fixpoint)
//! - Push/pop checkpoints for backtracking
//! - Persistent inputs for learned clauses

mod cause_sink;
pub mod cnf;
mod conflict_analysis;
mod literal_counts_sink;
pub mod proof;
mod queries;
mod solve;
mod solver;
mod solver_setup;
mod types;

#[cfg(test)]
mod tests;

pub use cnf::{Cnf, SolveResult};
pub use solver::Solver;
pub use types::{ClauseId, Conflict, Level, Lit, Var};

use contiguous_data::L2Multiset;

/// A sink that tracks assignments with efficient lookup by literal.
pub type AssignmentsSink = L2Multiset<Lit, Level>;
