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

    /// Get the next unassigned variable (simple heuristic: lowest numbered).
    pub fn pick_branching_variable(&self) -> Option<Var> {
        let assigned = self.outputs.assigned.get();
        for v in 1..=self.state.num_vars.raw() {
            let var = Var::new(v);
            if !assigned.contains(&Lit::pos(var)) && !assigned.contains(&Lit::neg(var)) {
                return Some(var);
            }
        }
        None
    }
}
