//! Main solve loop for the CDCL solver.

use super::Solver;
use super::types::Lit;

impl Solver {
    /// Main solve loop with CDCL (Conflict-Driven Clause Learning).
    ///
    /// Uses 1-UIP conflict analysis to learn clauses and perform
    /// non-chronological backtracking.
    pub fn solve(&mut self) -> bool {
        loop {
            // Propagate
            match self.propagate() {
                Ok(()) => {
                    // No conflict - pick next variable or return SAT
                    match self.pick_branching_variable() {
                        Some(v) => {
                            // Decide: try positive literal first
                            self.decide(Lit::pos(v));
                        }
                        None => {
                            // All variables assigned, no conflict = SAT
                            return true;
                        }
                    }
                }
                Err(conflict) => {
                    // Conflict! Analyze and learn.
                    match self.analyze_conflict(conflict) {
                        None => {
                            // Conflict at level 0 = UNSAT
                            return false;
                        }
                        Some(analysis) => {
                            // Learn the clause
                            self.learn_clause(&analysis.learned_clause);

                            // Non-chronological backtrack to the computed level
                            self.backtrack_to(analysis.backtrack_level);

                            // The learned clause is now unit (asserting), so propagation
                            // will assign the UIP literal on the next iteration
                        }
                    }
                }
            }
        }
    }
}
