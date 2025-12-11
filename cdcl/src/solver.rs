//! CDCL SAT Solver structure and methods.

use contiguous_data::{HashMap, L2Multiset};
use relational::database::{
    Database, InputHandle, Output, PersistentInputHandle, SavedOutput, SavedRelation,
};

use super::clause_deletion::ClauseDeletion;
use super::conflicts_sink::ConflictsSink;
use super::restart::RestartStrategy;
use super::types::{ConstraintId, Conflict, Level, Lit, Var, Weight};
use super::vsids::Vsids;

/// Type aliases for outputs with custom sinks.
type ConflictsOutput = Output<Conflict, ConflictsSink>;

/// Input handles for the solver.
pub(super) struct Inputs {
    /// PB constraint terms: (constraint_id, literal, weight) - persistent
    pub(crate) terms: PersistentInputHandle<(ConstraintId, Lit, Weight)>,
    /// PB constraint bounds: (constraint_id, bound) - persistent
    pub(crate) bounds: PersistentInputHandle<(ConstraintId, Weight)>,
    /// Learned constraint terms (persistent - survive backtracking)
    pub(crate) learned_terms: PersistentInputHandle<(ConstraintId, Lit, Weight)>,
    /// Learned constraint bounds (persistent - survive backtracking)
    pub(crate) learned_bounds: PersistentInputHandle<(ConstraintId, Weight)>,
    /// Decision levels - we insert the current level here
    pub(crate) levels: InputHandle<Level>,
    /// Decision assignments (lit, level) - inserted directly for decisions
    pub(crate) decision_assignments: InputHandle<(Lit, Level)>,
    /// Analysis input for conflict analysis
    pub(crate) analysis: InputHandle<Conflict>,
}

/// Output relations from the dataflow.
pub(super) struct Outputs {
    /// Assigned literals (derived from feedback variable).
    pub(crate) assigned: SavedOutput<Lit>,
    /// Saved relation for assigned literals - use `.get()` to get a relation for external use.
    pub(crate) assigned_saved: SavedRelation<Lit>,
    /// Assignment levels: (literal, level) for each assigned literal.
    pub(crate) assignment_levels: SavedOutput<(Lit, Level), L2Multiset<Lit, Level>>,
    /// Learned constraint literals for conflict analysis: (literal, level).
    pub(crate) new_clause: Output<(Lit, Level)>,
    /// Constraint IDs used during conflict analysis (for activity bumping).
    pub(crate) analysis_constraint_ids: Output<ConstraintId>,
    /// Conflicts detected during propagation.
    pub(crate) conflicts: ConflictsOutput,
    /// Assignments at the current decision level for VSIDS phase saving.
    pub(crate) this_level_assignments: Output<Lit>,
    /// Learned constraint terms for deletion.
    pub(crate) learned_terms: Output<(ConstraintId, Lit, Weight)>,
    /// Learned constraint bounds for deletion.
    pub(crate) learned_bounds: Output<(ConstraintId, Weight)>,
}

/// Solver state that doesn't involve the dataflow.
pub(super) struct State {
    /// Current decision level (local copy for convenience).
    pub(crate) current_level: Level,
    /// Next ID for learned clauses.
    pub(crate) next_learned_id: u32,
    /// Restart strategy.
    pub(crate) restart: RestartStrategy,
    /// Clause deletion manager.
    pub(crate) clause_deletion: ClauseDeletion,
    /// VSIDS decision heuristic.
    pub(crate) vsids: Vsids,
    /// Whether an empty clause has been added (immediate UNSAT).
    pub(crate) has_empty_clause: bool,
}

/// CDCL SAT Solver.
pub struct Solver {
    pub(super) inputs: Inputs,
    pub(super) outputs: Outputs,
    pub(super) state: State,
}

impl Solver {
    /// Add a clause (PB constraint with all weights=1, bound=1) to the solver.
    pub fn add_clause(&mut self, db: &mut Database, id: u32, literals: &[Lit]) {
        // Empty clause means immediate UNSAT
        if literals.is_empty() {
            self.state.has_empty_clause = true;
            return;
        }
        let cid = ConstraintId::Original(id);
        for &lit in literals {
            self.inputs.terms.insert((cid, lit, 1));
        }
        self.inputs.bounds.insert((cid, 1));
        db.commit();
    }

    /// Add a PB constraint: sum of (lit * weight) >= bound.
    pub fn add_pb_constraint(
        &mut self,
        db: &mut Database,
        id: u32,
        terms: &[(Lit, Weight)],
        bound: Weight,
    ) {
        let cid = ConstraintId::Original(id);
        // Check if trivially unsat (bound > sum of all weights)
        let total_weight: Weight = terms.iter().map(|(_, w)| w).sum();
        if bound > total_weight {
            self.state.has_empty_clause = true;
            return;
        }
        // Check if trivially sat (bound == 0)
        if bound == 0 {
            return; // Always satisfied, don't add
        }
        for &(lit, weight) in terms {
            self.inputs.terms.insert((cid, lit, weight));
        }
        self.inputs.bounds.insert((cid, bound));
        db.commit();
    }

    /// Make a decision: assign a literal at a new decision level.
    pub(super) fn decide_internal(&mut self, db: &mut Database, lit: Lit) {
        db.push();
        self.state.current_level.inc();

        self.inputs.levels.insert(self.state.current_level);
        self.inputs
            .decision_assignments
            .insert((lit, self.state.current_level));
        db.commit();
    }

    /// Make a decision: assign a literal at a new decision level.
    pub(crate) fn decide(&mut self, db: &mut Database, lit: Lit) {
        self.decide_internal(db, lit);
    }

    /// Propagate units until fixpoint or conflict.
    /// Returns Ok(()) if no conflict, Err(conflict) if conflict found.
    pub(crate) fn propagate(&mut self) -> Result<(), Conflict> {
        if let Some(conflict) = self.outputs.conflicts.get().first() {
            return Err(conflict);
        }
        Ok(())
    }

    /// Backtrack to the given level, popping decision stack entries.
    pub(crate) fn backtrack_to(&mut self, db: &mut Database, level: Level) {
        while self.state.current_level > level {
            // Track which variables we've seen and their polarity.
            // None means conflict (both polarities seen).
            let mut seen: HashMap<Var, Option<bool>> = HashMap::default();
            // Sort literals for deterministic processing order.
            let assignments = self.outputs.this_level_assignments.get();
            let mut lits: Vec<_> = assignments.iter().copied().collect();
            lits.sort();
            for lit in lits {
                let var = lit.var();
                let positive = lit.is_positive();
                match seen.get(&var) {
                    None => {
                        seen.insert(var, Some(positive));
                    }
                    Some(Some(prev)) if *prev != positive => {
                        // Conflict: saw both polarities
                        seen.insert(var, None);
                    }
                    _ => {} // Same polarity again, no change
                }
            }

            let mut seen = seen.into_iter().collect::<Vec<_>>();
            // Sort for deterministic order
            seen.sort();

            // Set phases: conflict -> false, otherwise use polarity
            for (var, polarity) in seen {
                self.state.vsids.set_phase(var, polarity.unwrap_or(false));
            }

            let popped = db.pop();
            assert!(popped, "Tried to backtrack past level 0");
            self.state.current_level.dec();
        }
        // Trigger propagation after backtracking to pick up any unit learned clauses
        db.commit();
    }

    /// Learn a clause with level information for LBD tracking.
    /// Learned clauses are PB constraints with all weights=1, bound=1.
    pub(crate) fn learn_clause(&mut self, db: &mut Database, clause: &[(Lit, Level)]) -> ConstraintId {
        let id = self.state.next_learned_id;
        self.state.next_learned_id += 1;
        let cid = ConstraintId::Learned(id);
        for &(lit, _) in clause {
            self.inputs.learned_terms.insert((cid, lit, 1));
        }
        self.inputs.learned_bounds.insert((cid, 1));
        if clause.is_empty() {
            self.state.has_empty_clause = true;
        } else {
            // Track for clause deletion
            self.state.clause_deletion.on_learn(cid, clause);
        }
        db.commit();
        cid
    }

    /// Restart: backtrack to level 0, clearing all decisions.
    pub(crate) fn restart(&mut self, db: &mut Database) {
        self.backtrack_to(db, Level::TOP);
        self.state.restart.on_restart();
    }

    /// Perform clause deletion if the learned constraint database is too large.
    pub(crate) fn maybe_delete_clauses(&mut self, db: &mut Database) {
        if self.state.clause_deletion.should_delete() {
            let to_delete = self.state.clause_deletion.select_for_deletion();
            let to_delete_set: std::collections::HashSet<_> = to_delete.iter().copied().collect();

            // Collect terms and bounds to delete
            let terms_to_delete: Vec<_> = self.outputs.learned_terms.get()
                .iter()
                .filter(|(cid, _, _)| to_delete_set.contains(cid))
                .copied()
                .collect();
            let bounds_to_delete: Vec<_> = self.outputs.learned_bounds.get()
                .iter()
                .filter(|(cid, _)| to_delete_set.contains(cid))
                .copied()
                .collect();

            for (cid, lit, weight) in terms_to_delete {
                self.inputs.learned_terms.delete((cid, lit, weight));
            }
            for (cid, bound) in bounds_to_delete {
                self.inputs.learned_bounds.delete((cid, bound));
            }
            db.commit();
        }
    }

    /// Get a reference to the saved assigned relation.
    /// Use `.get()` on the returned SavedRelation to get a Relation for external use.
    pub fn assigned_saved(&self) -> &SavedRelation<Lit> {
        &self.outputs.assigned_saved
    }

    /// Get the decision level at which a literal was assigned.
    /// Returns None if the literal is not currently assigned.
    pub fn get_level(&self, lit: Lit) -> Option<Level> {
        let levels = self.outputs.assignment_levels.get();
        let mut iter = levels.iter_values(&lit);
        let level = iter.next().copied();
        assert!(iter.next().is_none(), "Literal assigned at multiple levels");
        level
    }
}
