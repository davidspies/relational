//! CDCL SAT Solver structure and methods.

use relational::database2::{CommitId, Database2, InputHandle, Output, PersistentInputHandle};

use super::types::{ClauseId, Conflict, Level, Lit, Var};

/// CDCL SAT Solver.
pub struct Solver {
    pub(super) db: Database2,

    // === Input Handles ===
    /// Original clauses: (clause_id, literal)
    pub(super) clauses: InputHandle<(ClauseId, Lit)>,

    /// Learned clauses (persistent - survive backtracking)
    pub(super) learned: PersistentInputHandle<(ClauseId, Lit)>,

    /// Decision levels - we insert the current level here
    pub(super) levels: InputHandle<Level>,

    /// Decision assignments (lit, level, clause_id) - inserted directly for decisions
    pub(super) decision_assignments: InputHandle<(Lit, Level, ClauseId)>,

    // === Output Relations ===
    /// Final assignments: (lit, level) - derived by taking min commit_id per lit
    pub(super) assignments: Output<(Lit, Level)>,

    /// Causes: ((lit, commit_id), (clause_id, level))
    pub(super) causes: Output<((Lit, CommitId), (ClauseId, Level))>,

    /// The "assigned" relation - just tracks which literals are assigned true
    pub(super) assigned: Output<Lit>,

    /// Unit clauses that need propagation: (clause_id, implied_literal)
    pub(super) units: Output<(ClauseId, Lit)>,

    /// Conflicts detected during propagation
    pub(super) conflicts: Output<Conflict>,

    // === Solver State ===
    /// Current decision level (local copy for convenience).
    pub(super) current_level: Level,

    /// Next clause ID for learned clauses.
    pub(super) next_learned_id: ClauseId,

    /// Number of variables.
    pub(super) num_vars: Var,

    /// Stack of decisions: (level, literal, tried_both)
    pub(super) decision_stack: Vec<(Level, Lit, bool)>,
}

impl Solver {
    /// Add an original clause to the solver.
    pub fn add_clause(&mut self, clause_id: ClauseId, literals: &[Lit]) {
        for &lit in literals {
            self.clauses.insert((clause_id, lit));
        }
        self.db.commit();
    }

    /// Make a decision: assign a literal at a new decision level.
    /// `tried_opposite` indicates if we've already tried the opposite polarity.
    pub(super) fn decide_internal(&mut self, lit: Lit, tried_opposite: bool) {
        self.db.push();
        self.current_level.inc();
        self.decision_stack
            .push((self.current_level, lit, tried_opposite));

        self.levels.insert(self.current_level);
        self.decision_assignments
            .insert((lit, self.current_level, ClauseId::DECISION));
        self.db.commit();
    }

    /// Make a decision: assign a literal at a new decision level.
    pub fn decide(&mut self, lit: Lit) {
        self.decide_internal(lit, false);
    }

    /// Propagate units until fixpoint or conflict.
    /// Returns Ok(()) if no conflict, Err(conflict) if conflict found.
    pub fn propagate(&mut self) -> Result<(), Conflict> {
        let conflicts: Vec<_> = self.conflicts.collect();
        if let Some(&conflict) = conflicts.first() {
            return Err(conflict);
        }
        Ok(())
    }

    /// Backtrack to the given level, popping decision stack entries.
    pub fn backtrack_to(&mut self, level: Level) {
        while self.current_level > level {
            self.db.pop();
            self.decision_stack.pop();
            self.current_level.dec();
        }
    }

    /// Learn a clause (adds to persistent learned relation).
    pub fn learn_clause(&mut self, literals: &[Lit]) -> ClauseId {
        let cid = self.next_learned_id;
        self.next_learned_id = ClauseId::new(self.next_learned_id.raw() + 1);
        for &lit in literals {
            self.learned.insert((cid, lit));
        }
        self.db.commit();
        cid
    }
}
