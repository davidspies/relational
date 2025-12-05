//! Query methods for the CDCL solver.

use super::Solver;
use super::types::{Lit, Var};

impl Solver {
    /// Get the truth value of a variable, if assigned.
    pub fn value(&self, v: Var) -> Option<bool> {
        let assigned = self.outputs.causes.get();
        if assigned.contains_lit(Lit::pos(v)) {
            Some(true)
        } else if assigned.contains_lit(Lit::neg(v)) {
            Some(false)
        } else {
            None
        }
    }

    /// Pick the next branching literal using VSIDS heuristic with phase saving.
    pub(crate) fn pick_branching_literal(&mut self) -> Option<Lit> {
        let assigned = self.outputs.causes.get();
        let var = self
            .state
            .vsids
            .pick(|v| assigned.contains_lit(Lit::pos(v)) || assigned.contains_lit(Lit::neg(v)));
        var.map(|v| {
            let phase = self.state.vsids.get_phase(v);
            if phase { Lit::pos(v) } else { Lit::neg(v) }
        })
    }
}
