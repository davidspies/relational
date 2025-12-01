//! Main solve loop for the CDCL solver.

use super::Solver;
use super::proof::ProofWriter;
use super::types::Lit;

impl Solver {
    /// Main solve loop with CDCL (Conflict-Driven Clause Learning).
    ///
    /// Uses 1-UIP conflict analysis to learn clauses and perform
    /// non-chronological backtracking.
    pub fn solve(&mut self) -> bool {
        self.solve_with_proof(None)
    }

    /// Solve with optional DRAT proof logging.
    pub fn solve_with_proof(&mut self, proof: Option<&mut ProofWriter>) -> bool {
        self.solve_internal(proof)
    }

    fn solve_internal(&mut self, mut proof: Option<&mut ProofWriter>) -> bool {
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
                            if let Some(ref mut p) = proof {
                                let _ = p.add_empty_clause();
                                let _ = p.flush();
                            }
                            return false;
                        }
                        Some(analysis) => {
                            // Log the learned clause to proof
                            if let Some(ref mut p) = proof {
                                let _ = p.add_clause(&analysis.learned_clause);
                            }

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
