//! Main solve loop for the CDCL solver.

use super::Solver;
use super::types::{Level, Lit, neg};

impl Solver {
    /// Main solve loop.
    pub(crate) fn solve(&mut self) -> bool {
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
                Err(_conflict_clause) => {
                    // Conflict! Need to backtrack.
                    // Find a decision level where we haven't tried both polarities.
                    loop {
                        if self.current_level == Level::TOP {
                            // Conflict at level 0 = UNSAT
                            return false;
                        }

                        // Get the decision at current level
                        let (_, decision_lit, tried_both) =
                            self.decision_stack.last().copied().unwrap();

                        // Calculate previous level
                        let prev_level = Level::new(self.current_level.raw().saturating_sub(1));

                        if tried_both {
                            // Already tried both polarities at this level, backtrack further
                            self.backtrack_to(prev_level);
                        } else {
                            // Haven't tried opposite polarity yet
                            // Backtrack this level and try the opposite
                            self.backtrack_to(prev_level);
                            self.decide_internal(neg(decision_lit), true);
                            break;
                        }
                    }
                }
            }
        }
    }
}
