//! ASP (Answer Set Programming) solver built on CDCL SAT solver.
//!
//! Uses a two-solver architecture:
//! - Candidate solver: finds candidate answer sets
//! - Check solver: checks supportedness (looks for smaller models of the positive reduct)

pub mod encoding;
pub mod parser;
mod solver;
pub mod types;

pub use parser::parse_smodels;
pub use solver::AspSolver;
pub use types::{Atom, Program, Rule};
