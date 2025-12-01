//! CDCL SAT Solver structure and methods.

use std::collections::HashMap;

use relational::database::{
    CommitId, Database, InputHandle, Output, PersistentInputHandle, SavedOutput,
};

use super::assignments_sink::AssignmentsSink;
use super::cause_sink::CauseSink;
use super::types::{ClauseId, Conflict, Level, Lit, Var};

/// Type alias for the causes output (complex due to nested structure).
type CausesOutput = Output<((Lit, CommitId), (ClauseId, Level)), CauseSink>;
type AssignmentsOutput = SavedOutput<(Lit, Level), AssignmentsSink>;

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
    /// Final assignments: (lit, level) - derived by taking min commit_id per lit
    pub assignments: AssignmentsOutput,
    /// Causes: ((lit, commit_id), (clause_id, level)) with CauseSink for efficient lookup
    pub causes: CausesOutput,
    /// The "assigned" relation - just tracks which literals are assigned true
    pub assigned: SavedOutput<Lit>,
    /// Conflicts detected during propagation
    pub conflicts: SavedOutput<Conflict>,
}

/// Solver state that doesn't involve the dataflow.
pub(super) struct State {
    /// Current decision level (local copy for convenience).
    pub current_level: Level,
    /// Next clause ID for learned clauses.
    pub next_learned_id: ClauseId,
    /// Number of variables.
    pub num_vars: Var,
    /// Stack of decisions: (level, literal, tried_both)
    pub decision_stack: Vec<(Level, Lit, bool)>,
    /// Cache of clause contents: clause_id -> list of literals
    pub clause_db: HashMap<ClauseId, Vec<Lit>>,
}

/// CDCL SAT Solver.
pub struct Solver {
    pub(super) db: Database,
    pub(super) inputs: Inputs,
    pub(super) outputs: Outputs,
    pub(super) state: State,
}

impl Solver {
    /// Add an original clause to the solver.
    pub fn add_clause(&mut self, clause_id: ClauseId, literals: &[Lit]) {
        for &lit in literals {
            self.inputs.clauses.insert((clause_id, lit));
        }
        // Cache clause contents for conflict analysis
        self.state.clause_db.insert(clause_id, literals.to_vec());
        // Ensure learned clause IDs don't overlap with original clause IDs
        if clause_id >= self.state.next_learned_id {
            self.state.next_learned_id = ClauseId::new(clause_id.raw() + 1);
        }
        self.db.commit();
    }

    /// Make a decision: assign a literal at a new decision level.
    /// `tried_opposite` indicates if we've already tried the opposite polarity.
    pub(super) fn decide_internal(&mut self, lit: Lit, tried_opposite: bool) {
        self.db.push();
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
        self.db.commit();
    }

    /// Make a decision: assign a literal at a new decision level.
    pub fn decide(&mut self, lit: Lit) {
        self.decide_internal(lit, false);
    }

    /// Propagate units until fixpoint or conflict.
    /// Returns Ok(()) if no conflict, Err(conflict) if conflict found.
    pub fn propagate(&mut self) -> Result<(), Conflict> {
        if let Some(&conflict) = self.outputs.conflicts.get().iter().next() {
            return Err(conflict);
        }
        Ok(())
    }

    /// Backtrack to the given level, popping decision stack entries.
    pub fn backtrack_to(&mut self, level: Level) {
        while self.state.current_level > level {
            let popped = self.db.pop();
            assert!(popped, "Tried to backtrack past level 0");
            self.state.decision_stack.pop();
            self.state.current_level.dec();
        }
        // Trigger propagation after backtracking to pick up any unit learned clauses
        self.db.commit();
    }

    /// Learn a clause (adds to persistent learned relation).
    pub fn learn_clause(&mut self, literals: &[Lit]) -> ClauseId {
        let cid = self.state.next_learned_id;
        self.state.next_learned_id = ClauseId::new(self.state.next_learned_id.raw() + 1);
        for &lit in literals {
            self.inputs.learned.insert((cid, lit));
        }
        // Cache clause contents for conflict analysis
        self.state.clause_db.insert(cid, literals.to_vec());
        self.db.commit();
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
    pub fn graph(&self) -> relational::database::GraphHandle {
        self.db.graph()
    }
}
