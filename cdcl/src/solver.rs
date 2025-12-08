//! CDCL SAT Solver structure and methods.

use contiguous_data::{HashMap, L2Multiset};
use relational::database::{
    Database, InputHandle, Output, PersistentInputHandle, SavedOutput, SavedRelation,
};

use super::clause_deletion::ClauseDeletion;
use super::conflicts_sink::ConflictsSink;
use super::restart::RestartStrategy;
use super::types::{ClauseId, Conflict, Level, Lit, Var};
use super::vsids::Vsids;

/// Type aliases for outputs with custom sinks.
type ConflictsOutput = Output<Conflict, ConflictsSink>;
type LearnedClausesOutput = Output<(ClauseId, Lit), L2Multiset<ClauseId, Lit>>;

/// Input handles for the solver.
pub(super) struct Inputs {
    /// Original clauses: (clause_id, literal) - persistent to survive backtracking
    pub(crate) clauses: PersistentInputHandle<(ClauseId, Lit)>,
    /// Learned clauses (persistent - survive backtracking)
    pub(crate) learned: PersistentInputHandle<(ClauseId, Lit)>,
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
    /// Learned clause literals for conflict analysis: (literal, level).
    pub(crate) new_clause: Output<(Lit, Level)>,
    /// Clause IDs used during conflict analysis (for activity bumping).
    pub(crate) analysis_clause_ids: Output<ClauseId>,
    /// Conflicts detected during propagation.
    pub(crate) conflicts: ConflictsOutput,
    /// Assignments at the current decision level for VSIDS phase saving.
    pub(crate) this_level_assignments: Output<Lit>,
    /// Learned clauses indexed by clause_id for deletion.
    pub(crate) learned_clauses: LearnedClausesOutput,
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
    /// Add an original clause to the solver.
    pub fn add_clause(&mut self, db: &mut Database, id: u32, literals: &[Lit]) {
        // Empty clause means immediate UNSAT
        if literals.is_empty() {
            self.state.has_empty_clause = true;
            return;
        }
        let clause_id = ClauseId::Original(id);
        for &lit in literals {
            self.inputs.clauses.insert((clause_id, lit));
        }
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
    pub(crate) fn learn_clause(&mut self, db: &mut Database, clause: &[(Lit, Level)]) -> ClauseId {
        let id = self.state.next_learned_id;
        self.state.next_learned_id += 1;
        let cid = ClauseId::Learned(id);
        for &(lit, _) in clause {
            self.inputs.learned.insert((cid, lit));
        }
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

    /// Perform clause deletion if the learned clause database is too large.
    pub(crate) fn maybe_delete_clauses(&mut self, db: &mut Database) {
        if self.state.clause_deletion.should_delete() {
            let to_delete = self.state.clause_deletion.select_for_deletion();
            let learned_clauses = self.outputs.learned_clauses.get();
            for clause_id in to_delete {
                for &lit in learned_clauses.iter_values(&clause_id) {
                    self.inputs.learned.delete((clause_id, lit));
                }
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
