//! CDCL SAT Solver structure and methods.

use std::collections::HashMap;

use relational::database::{CommitId, Database, InputHandle, Output, PersistentInputHandle};

use super::cause_sink::CauseSink;
use super::clause_deletion::ClauseDeletion;
use super::conflicts_sink::ConflictsSink;
use super::restart::RestartStrategy;
use super::types::{ClauseId, Conflict, Level, Lit, Var};
use super::vsids::Vsids;

/// Type aliases for outputs with custom sinks.
type CausesOutput = Output<((Lit, CommitId), (ClauseId, Level)), CauseSink>;
type ConflictsOutput = Output<Conflict, ConflictsSink>;

/// Input handles for the solver.
pub(super) struct Inputs {
    /// Original clauses: (clause_id, literal)
    pub clauses: InputHandle<(ClauseId, Lit)>,
    /// Learned clauses (persistent - survive backtracking)
    pub learned: PersistentInputHandle<(ClauseId, Lit)>,
    /// Decision levels - we insert the current level here
    pub levels: InputHandle<Level>,
    /// Decision assignments (lit, level, clause_id) - inserted directly for decisions
    pub decision_assignments: InputHandle<(Lit, Level, ClauseId)>,
}

/// Output relations from the dataflow.
pub(super) struct Outputs {
    /// Causes: ((lit, commit_id), (clause_id, level)) with CauseSink for efficient lookup
    pub causes: CausesOutput,
    /// Conflicts detected during propagation
    pub conflicts: ConflictsOutput,
    /// Assignments at the current decision level. Positive counts indicate assigned true,
    /// negative indicate assigned false. Conflict literals are omitted.
    pub this_level_assignments: Output<Lit>,
}

/// Solver state that doesn't involve the dataflow.
pub(super) struct State {
    /// Current decision level (local copy for convenience).
    pub current_level: Level,
    /// Next clause ID for learned clauses.
    pub next_learned_id: ClauseId,
    /// Stack of decisions: (level, literal, tried_both)
    pub decision_stack: Vec<(Level, Lit, bool)>,
    /// Cache of clause contents: clause_id -> list of literals
    pub clause_db: HashMap<ClauseId, Vec<Lit>>,
    /// Restart strategy.
    pub restart: RestartStrategy,
    /// Clause deletion manager.
    pub clause_deletion: ClauseDeletion,
    /// VSIDS decision heuristic.
    pub vsids: Vsids,
    /// Total number of variables.
    pub num_vars: u32,
}

/// CDCL SAT Solver.
pub struct Solver {
    pub(super) inputs: Inputs,
    pub(super) outputs: Outputs,
    pub(super) state: State,
}

impl Solver {
    /// Add an original clause to the solver.
    pub fn add_clause(&mut self, db: &mut Database, clause_id: ClauseId, literals: &[Lit]) {
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
    /// `tried_opposite` indicates if we've already tried the opposite polarity.
    pub(super) fn decide_internal(&mut self, db: &mut Database, lit: Lit, tried_opposite: bool) {
        db.push();
        self.state.current_level.inc();
        self.state
            .decision_stack
            .push((self.state.current_level, lit, tried_opposite));

        self.inputs.levels.insert(self.state.current_level);
        self.inputs.decision_assignments.insert((
            lit,
            self.state.current_level,
            ClauseId::DECISION,
        ));
        db.commit();
    }

    /// Make a decision: assign a literal at a new decision level.
    pub fn decide(&mut self, db: &mut Database, lit: Lit) {
        self.decide_internal(db, lit, false);
    }

    /// Propagate units until fixpoint or conflict.
    /// Returns Ok(()) if no conflict, Err(conflict) if conflict found.
    pub fn propagate(&mut self) -> Result<(), Conflict> {
        if let Some(conflict) = self.outputs.conflicts.get().first() {
            return Err(conflict);
        }
        Ok(())
    }

    /// Backtrack to the given level, popping decision stack entries.
    pub fn backtrack_to(&mut self, db: &mut Database, level: Level) {
        while self.state.current_level > level {
            // Track which variables we've seen and their polarity.
            // None means conflict (both polarities seen).
            let mut seen: HashMap<Var, Option<bool>> = HashMap::new();
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
            self.state.decision_stack.pop();
            self.state.current_level.dec();
        }
        // Trigger propagation after backtracking to pick up any unit learned clauses
        db.commit();
    }

    /// Learn a clause (adds to persistent learned relation).
    pub fn learn_clause(&mut self, db: &mut Database, literals: &[Lit]) -> ClauseId {
        self.learn_clause_with_levels(db, literals, &[])
    }

    /// Learn a clause with level information for LBD tracking.
    pub fn learn_clause_with_levels(
        &mut self,
        db: &mut Database,
        literals: &[Lit],
        levels: &[Level],
    ) -> ClauseId {
        let cid = self.state.next_learned_id;
        self.state.next_learned_id = ClauseId::new(self.state.next_learned_id.raw() + 1);
        for &lit in literals {
            self.inputs.learned.insert((cid, lit));
        }
        // Cache clause contents for conflict analysis
        self.state.clause_db.insert(cid, literals.to_vec());
        // Track for clause deletion if we have levels
        if !levels.is_empty() {
            self.state.clause_deletion.on_learn(cid, literals, levels);
        }
        db.commit();
        cid
    }

    /// Get the literals in a clause.
    pub fn get_clause(&self, clause_id: ClauseId) -> Option<&[Lit]> {
        self.state.clause_db.get(&clause_id).map(|v| v.as_slice())
    }

    /// Get a handle to the dataflow graph for debugging/visualization.
    ///
    /// The returned handle is thread-safe and can be sent to another thread
    /// (e.g., for a ctrl-C handler to dump the graph).
    pub fn graph(db: &Database) -> relational::database::GraphHandle {
        db.graph()
    }

    /// Restart: backtrack to level 0, clearing all decisions.
    pub fn restart(&mut self, db: &mut Database) {
        self.backtrack_to(db, Level::TOP);
        self.state.restart.on_restart();
    }

    /// Delete a learned clause from the solver.
    pub fn delete_clause(&mut self, db: &mut Database, clause_id: ClauseId) {
        if let Some(literals) = self.state.clause_db.remove(&clause_id) {
            for lit in literals {
                self.inputs.learned.delete((clause_id, lit));
            }
            self.state.clause_deletion.remove(clause_id);
            db.commit();
        }
    }

    /// Perform clause deletion if the learned clause database is too large.
    pub fn maybe_delete_clauses(&mut self, db: &mut Database) {
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
