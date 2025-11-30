//! Query methods for the CDCL solver.

use std::collections::{BTreeMap, HashMap, HashSet};

use relational::Multiset;
use relational::database2::CommitId;

use super::Solver;
use super::types::{ClauseId, Conflict, Level, Lit, Var};

impl Solver {
    /// Get all currently assigned literals.
    pub(crate) fn get_assignments(&mut self) -> Vec<(Lit, Level)> {
        self.assignments.collect()
    }

    /// Get the current decision level.
    pub(crate) fn level(&self) -> Level {
        self.current_level
    }

    /// Check if a variable is assigned.
    pub(crate) fn is_assigned(&mut self, v: Var) -> bool {
        let assigned: HashSet<_> = self.assigned.collect().into_iter().collect();
        assigned.contains(&Lit::pos(v)) || assigned.contains(&Lit::neg(v))
    }

    /// Get the truth value of a variable, if assigned.
    pub(crate) fn value(&mut self, v: Var) -> Option<bool> {
        let assigned: HashSet<_> = self.assigned.collect().into_iter().collect();
        if assigned.contains(&Lit::pos(v)) {
            Some(true)
        } else if assigned.contains(&Lit::neg(v)) {
            Some(false)
        } else {
            None
        }
    }

    /// Get the next unassigned variable (simple heuristic: lowest numbered).
    pub(crate) fn pick_branching_variable(&mut self) -> Option<Var> {
        for v in 1..=self.num_vars.raw() {
            let var = Var::new(v);
            if !self.is_assigned(var) {
                return Some(var);
            }
        }
        None
    }

    /// Get current conflicts (for debugging).
    pub(crate) fn get_conflicts(&mut self) -> Vec<Conflict> {
        self.conflicts.collect()
    }

    /// Get current units (for debugging).
    pub(crate) fn get_units(&mut self) -> Vec<(ClauseId, Lit)> {
        self.units.collect()
    }

    /// Get the causes (implication graph) as a structured data type.
    /// Returns HashMap<Lit, BTreeMap<CommitId, Multiset<(ClauseId, Level)>>>
    /// For each literal, this maps each CommitId to the multiset of (ClauseId, Level) that derived it at that commit.
    pub(crate) fn get_causes(&mut self) -> HashMap<Lit, BTreeMap<CommitId, Multiset<(ClauseId, Level)>>> {
        let raw: Vec<((Lit, CommitId), (ClauseId, Level))> = self.causes.collect();
        let mut result: HashMap<Lit, BTreeMap<CommitId, Multiset<(ClauseId, Level)>>> =
            HashMap::new();

        for ((lit, commit_id), (clause_id, level)) in raw {
            result
                .entry(lit)
                .or_default()
                .entry(commit_id)
                .or_default()
                .insert((clause_id, level));
        }

        result
    }
}
