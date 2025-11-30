//! Query methods for the CDCL solver.

use std::cell::Ref;
use std::collections::HashSet;

use super::Solver;
use super::cause_sink::CauseSink;
use super::types::{ClauseId, Conflict, Level, Lit, Var};

impl Solver {
    /// Get all currently assigned literals.
    pub fn get_assignments(&self) -> Vec<(Lit, Level)> {
        self.outputs.assignments.collect()
    }

    /// Get the current decision level.
    pub fn level(&self) -> Level {
        self.state.current_level
    }

    /// Check if a variable is assigned.
    pub fn is_assigned(&self, v: Var) -> bool {
        let assigned: HashSet<_> = self.outputs.assigned.collect().into_iter().collect();
        assigned.contains(&Lit::pos(v)) || assigned.contains(&Lit::neg(v))
    }

    /// Get the truth value of a variable, if assigned.
    pub fn value(&self, v: Var) -> Option<bool> {
        let assigned: HashSet<_> = self.outputs.assigned.collect().into_iter().collect();
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
        for v in 1..=self.state.num_vars.raw() {
            let var = Var::new(v);
            if !self.is_assigned(var) {
                return Some(var);
            }
        }
        None
    }

    /// Get current conflicts (for debugging).
    pub fn get_conflicts(&self) -> Vec<Conflict> {
        self.outputs.conflicts.collect()
    }

    /// Get current units (for debugging).
    pub fn get_units(&self) -> Vec<(ClauseId, Lit)> {
        self.outputs.units.collect()
    }

    /// Get a reference to the causes (implication graph).
    pub fn get_causes(&self) -> Ref<'_, CauseSink> {
        self.outputs.causes.get()
    }
}
