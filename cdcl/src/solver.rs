//! CDCL SAT Solver structure and methods.

use ahash::AHashMap;
use relational::database::{Database, InputHandle, Output, PersistentInputHandle, SavedOutput};

use super::clause_deletion::ClauseDeletion;
use super::conflicts_sink::ConflictsSink;
use super::restart::RestartStrategy;
use super::types::{ClauseId, Conflict, Level, Lit, Var};
use super::vsids::Vsids;

/// Type aliases for outputs with custom sinks.
type ConflictsOutput = Output<Conflict, ConflictsSink>;

/// Input handles for the solver.
pub(super) struct Inputs {
    /// Original clauses: (clause_id, literal)
    pub(crate) clauses: InputHandle<(ClauseId, Lit)>,
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
    /// Learned clause literals for conflict analysis: (literal, level).
    pub(crate) new_clause: Output<(Lit, Level)>,
    /// Conflicts detected during propagation.
    pub(crate) conflicts: ConflictsOutput,
    /// Assignments at the current decision level for VSIDS phase saving.
    pub(crate) this_level_assignments: Output<Lit>,
}

/// Solver state that doesn't involve the dataflow.
pub(super) struct State {
    /// Current decision level (local copy for convenience).
    pub(crate) current_level: Level,
    /// Next clause ID for learned clauses.
    pub(crate) next_learned_id: ClauseId,
    /// Cache of clause contents: clause_id -> list of literals
    pub(crate) clause_db: AHashMap<ClauseId, Vec<Lit>>,
    /// Restart strategy.
    pub(crate) restart: RestartStrategy,
    /// Clause deletion manager.
    pub(crate) clause_deletion: ClauseDeletion,
    /// VSIDS decision heuristic.
    pub(crate) vsids: Vsids,
}

/// CDCL SAT Solver.
pub struct Solver {
    pub(super) inputs: Inputs,
    pub(super) outputs: Outputs,
    pub(super) state: State,
}

impl Solver {
    /// Add an original clause to the solver.
    pub(crate) fn add_clause(&mut self, db: &mut Database, clause_id: ClauseId, literals: &[Lit]) {
        for &lit in literals {
            self.inputs.clauses.insert((clause_id, lit));
        }
        // Cache clause contents for conflict analysis
        self.state.clause_db.insert(clause_id, literals.to_vec());
        // Ensure learned clause IDs don't overlap with original clause IDs
        if clause_id >= self.state.next_learned_id {
            self.state.next_learned_id = ClauseId::new(clause_id.raw() + 1);
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
            let mut seen: AHashMap<Var, Option<bool>> = AHashMap::new();
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
        let cid = self.state.next_learned_id;
        self.state.next_learned_id = ClauseId::new(self.state.next_learned_id.raw() + 1);
        let literals: Vec<Lit> = clause.iter().map(|&(lit, _)| lit).collect();
        for &lit in &literals {
            self.inputs.learned.insert((cid, lit));
        }
        // Cache clause contents for conflict analysis
        self.state.clause_db.insert(cid, literals);
        // Track for clause deletion
        self.state.clause_deletion.on_learn(cid, clause);
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
            for clause_id in to_delete {
                if let Some(literals) = self.state.clause_db.remove(&clause_id) {
                    for lit in literals {
                        self.inputs.learned.delete((clause_id, lit));
                    }
                }
            }
            db.commit();
        }
    }
}
