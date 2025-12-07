//! Query methods for the CDCL solver.

use super::Solver;
use super::types::Lit;

impl Solver {
    /// Pick the next branching literal using VSIDS heuristic with phase saving.
    pub(crate) fn pick_branching_literal(&mut self) -> Option<Lit> {
        let assigned = self.outputs.assigned.get();
        let var = self
            .state
            .vsids
            .pick(|v| assigned.contains(&Lit::pos(v)) || assigned.contains(&Lit::neg(v)));
        var.map(|v| {
            let phase = self.state.vsids.get_phase(v);
            if phase { Lit::pos(v) } else { Lit::neg(v) }
        })
    }
}
