//! Query methods for the CDCL solver.

use std::cell::Ref;

use super::Solver;
use super::assignments_sink::AssignmentsSink;
use super::types::{Level, Lit, Var};

impl Solver {
    /// Get a reference to the assignments sink.
    pub fn assignments(&self) -> Ref<'_, AssignmentsSink> {
        self.outputs.assignments.get()
    }

    /// Get the current decision level.
    pub fn level(&self) -> Level {
        self.state.current_level
    }

    /// Check if a variable is assigned.
    pub fn is_assigned(&self, v: Var) -> bool {
        let assigned = self.outputs.assigned.get();
        assigned.contains(&Lit::pos(v)) || assigned.contains(&Lit::neg(v))
    }

    /// Get the truth value of a variable, if assigned.
    pub fn value(&self, v: Var) -> Option<bool> {
        let assigned = self.outputs.assigned.get();
        if assigned.contains(&Lit::pos(v)) {
            Some(true)
        } else if assigned.contains(&Lit::neg(v)) {
            Some(false)
        } else {
            None
        }
    }

    /// Pick the next branching literal using the DLIS heuristic.
    ///
    /// Returns the unassigned literal that appears in the most remaining clauses.
    /// This tends to satisfy more clauses and prune the search space faster.
    pub fn pick_branching_literal(&self) -> Option<Lit> {
        let counts = self.outputs.literal_counts.get();
        let (_, lits) = counts.max_count()?;
        Some(lits.iter().next().copied().unwrap())
    }
}
