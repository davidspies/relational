//! Query methods for the CDCL solver.

use std::cell::Ref;

use crate::cause_sink::CauseSink;

use super::Solver;
use super::types::{Level, Lit, Var};

impl Solver {
    /// Get a reference to the assignments sink.
    pub fn assignments(&self) -> Ref<'_, CauseSink> {
        self.outputs.causes.get()
    }

    /// Get the current decision level.
    pub fn level(&self) -> Level {
        self.state.current_level
    }

    /// Check if a variable is assigned.
    pub fn is_assigned(&self, v: Var) -> bool {
        let assigned = self.outputs.causes.get();
        if assigned.contains_lit(Lit::pos(v)) || assigned.contains_lit(Lit::neg(v)) {
            return true;
        }
        let unary = self.outputs.unary_lits.get();
        unary.contains(&Lit::pos(v)) || unary.contains(&Lit::neg(v))
    }

    /// Get the truth value of a variable, if assigned.
    pub fn value(&self, v: Var) -> Option<bool> {
        // Check assignments from propagation
        let assigned = self.outputs.causes.get();
        if assigned.contains_lit(Lit::pos(v)) {
            Some(true)
        } else if assigned.contains_lit(Lit::neg(v)) {
            Some(false)
        } else {
            // Check unary literals from level 0 simplification
            let unary = self.outputs.unary_lits.get();
            if unary.contains(&Lit::pos(v)) {
                Some(true)
            } else if unary.contains(&Lit::neg(v)) {
                Some(false)
            } else {
                None
            }
        }
    }

    /// Pick the next branching literal using VSIDS heuristic with phase saving.
    pub fn pick_branching_literal(&mut self) -> Option<Lit> {
        let assigned = self.outputs.causes.get();
        let unary = self.outputs.unary_lits.get();
        let var = self.state.vsids.pick(|v| {
            assigned.contains_lit(Lit::pos(v))
                || assigned.contains_lit(Lit::neg(v))
                || unary.contains(&Lit::pos(v))
                || unary.contains(&Lit::neg(v))
        });
        var.map(|v| {
            let phase = self.state.vsids.get_phase(v);
            if phase { Lit::pos(v) } else { Lit::neg(v) }
        })
    }
}
