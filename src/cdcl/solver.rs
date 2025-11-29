//! CDCL SAT Solver implementation.

use crate::database::CommitId;
use crate::relation::Relation;
use crate::Database;

use super::types::{var, ClauseId, Conflict, Level, Lit, Var};

/// CDCL SAT Solver.
pub struct Solver {
    pub(super) db: Database,

    // === Input Relations ===
    /// Original clauses: (clause_id, literal)
    /// Each clause is represented as multiple tuples, one per literal.
    pub(super) clauses: Relation<(ClauseId, Lit)>,

    /// Learned clauses (persistent - survive backtracking)
    pub(super) learned: Relation<(ClauseId, Lit)>,

    /// Decision levels - we insert the current level here
    pub(super) levels: Relation<Level>,

    /// Decision assignments (lit, level, clause_id) - inserted directly for decisions
    pub(super) decision_assignments: Relation<(Lit, Level, ClauseId)>,

    // === Derived/State Relations ===
    /// Current level = max(levels)
    /// Note: Stored to keep the relation alive, accessed via the graph
    #[allow(dead_code)]
    pub(super) current_level_rel: Relation<Level>,

    /// Prep assignments from feedback_with_id: ((lit, level, clause_id), commit_id)
    /// This accumulates all discovered assignments with their discovery time and reason clause
    /// Note: Stored to keep the relation alive, accessed via the graph
    #[allow(dead_code)]
    pub(super) prep_assignments: Relation<((Lit, Level, ClauseId), CommitId)>,

    /// Final assignments: (lit, level) - derived by taking min commit_id per lit
    pub(super) assignments: Relation<(Lit, Level)>,

    /// Causes: ((lit, commit_id), (clause_id, level)) - tracks all ways each literal was derived
    /// Can be collected to HashMap<Lit, BTreeMap<CommitId, Multiset<(ClauseId, Level)>>>
    pub(super) causes: Relation<((Lit, CommitId), (ClauseId, Level))>,

    /// The "assigned" relation - just tracks which literals are assigned true
    pub(super) assigned: Relation<Lit>,

    /// Unit clauses that need propagation: (clause_id, implied_literal)
    pub(super) units: Relation<(ClauseId, Lit)>,

    /// Conflicts detected during propagation
    pub(super) conflicts: Relation<Conflict>,

    // === Solver State ===
    /// Current decision level (local copy for convenience).
    pub(super) current_level: Level,

    /// Next clause ID for learned clauses.
    pub(super) next_learned_id: ClauseId,

    /// Number of variables.
    pub(super) num_vars: Var,

    /// Stack of decisions: (level, literal, tried_both)
    /// tried_both = true means we've already tried the opposite polarity
    pub(super) decision_stack: Vec<(Level, Lit, bool)>,
}

impl Solver {
    /// Create a new solver for the given number of variables.
    pub fn new(num_vars: Var) -> Self {
        let mut db = Database::new();

        // === Input Relations ===
        let clauses = db.create_input::<(ClauseId, Lit)>("clauses");
        let learned = db.create_persistent_input::<(ClauseId, Lit)>("learned");

        // Levels input - we insert decision levels here
        let levels = db.create_input::<Level>("levels");

        // Decision assignments - inserted directly for decisions (with ClauseId::DECISION)
        let decision_assignments =
            db.create_input::<(Lit, Level, ClauseId)>("decision_assignments");

        // Current level = max(levels)
        let current_level_rel = db.max(levels);

        // All clauses (original + learned)
        let all_clauses = db.union(clauses, learned);

        // === Feedback-based Unit Propagation ===
        // prep_assignments accumulates ((Lit, Level, ClauseId), CommitId) via feedback_with_id
        let (prep_var, prep_assignments) =
            db.variable::<((Lit, Level, ClauseId), CommitId)>("prep_assignments");

        // Final assignments: for each literal, take the entry with minimum CommitId
        // group_min groups by lit, and for each lit picks the (level, commit_id) with min commit_id
        let assignments_with_id = db.group_min(
            prep_assignments,
            |((lit, _, _), _)| *lit,             // group by lit
            |((_, level, _), id)| (*level, *id), // value is (level, commit_id)
        );
        // Result is (Lit, (Level, CommitId)) - extract (Lit, Level)
        let assignments = db.map(assignments_with_id, |(lit, (level, _id))| (*lit, *level));

        // Causes: tracks all ways each literal was derived
        // Reshape prep_assignments from ((Lit, Level, ClauseId), CommitId) to ((Lit, CommitId), (ClauseId, Level))
        let causes = db.map(prep_assignments, |((lit, level, cid), commit_id)| {
            ((*lit, *commit_id), (*cid, *level))
        });

        // Derived: which literals are assigned true
        let assigned = db.map(assignments, |(lit, _)| *lit);

        // === Compute Units ===
        // Literals that are true (in assigned)
        let clause_lit_true = db.join(all_clauses, assigned, |(_, lit)| *lit, |lit| *lit);
        let satisfied_clauses = db.map(clause_lit_true, |((cid, _), _)| *cid);

        // Literals that are assigned (either true or false)
        let assigned_vars = db.map(assigned, |lit| var(*lit));

        // For each clause literal, check if its variable is assigned
        let clause_lit_with_var = db.map(all_clauses, |(cid, lit)| (*cid, *lit, var(*lit)));
        let clause_assigned_lits =
            db.join(clause_lit_with_var, assigned_vars, |(_, _, v)| *v, |v| *v);
        let clause_assigned_lit_ids =
            db.map(clause_assigned_lits, |((cid, lit, _), _)| (*cid, *lit));

        // Unassigned literals in clauses
        let clause_unassigned_lits = db.difference(all_clauses, clause_assigned_lit_ids);

        // Count unassigned literals per clause
        let unassigned_count = db.group_count(clause_unassigned_lits, |(cid, _)| *cid);

        // Clauses with exactly 1 unassigned literal
        let unit_candidate_clauses = db.filter(unassigned_count, |(_, count)| *count == 1);
        let unit_clause_ids = db.map(unit_candidate_clauses, |(cid, _)| *cid);

        // Get the unassigned literal for unit clauses
        let units_with_lit = db.join(
            unit_clause_ids,
            clause_unassigned_lits,
            |cid| *cid,
            |(cid, _)| *cid,
        );
        let potential_units = db.map(units_with_lit, |(cid, (_, lit))| (*cid, *lit));

        // Filter out satisfied clauses
        let satisfied_set = db.map(satisfied_clauses, |cid| *cid);
        let unit_clause_sat_check =
            db.join(potential_units, satisfied_set, |(cid, _)| *cid, |cid| *cid);
        let units_from_sat = db.map(unit_clause_sat_check, |((cid, lit), _)| (*cid, *lit));

        // Units = potential_units where clause is NOT satisfied
        let units = db.difference(potential_units, units_from_sat);

        // === Conflict Detection ===
        // Type 1: Clause conflicts (all literals false)
        let all_clause_ids = db.map(all_clauses, |(cid, _)| *cid);
        let all_clause_ids_distinct = db.distinct(all_clause_ids);
        let clauses_with_unassigned = db.map(unassigned_count, |(cid, _)| *cid);
        let fully_assigned_clauses =
            db.difference(all_clause_ids_distinct, clauses_with_unassigned);
        let satisfied_distinct = db.distinct(satisfied_set);
        let clause_conflicts = db.difference(fully_assigned_clauses, satisfied_distinct);

        // Type 2: Direct conflicts (both lit and neg(lit) assigned)
        // Map each assigned literal to its variable
        let assigned_with_var = db.map(assigned, |lit| (*lit, var(*lit)));
        // Join on variable to find pairs where both polarities are assigned
        let both_polarities = db.join(
            assigned_with_var,
            assigned_with_var,
            |(_, v)| *v,
            |(_, v)| *v,
        );
        // Filter to only pairs where the literals are different (one positive, one negative)
        let conflicting_pairs = db.filter(both_polarities, |((lit1, _), (lit2, _))| lit1 != lit2);
        // Extract the variable that has both polarities assigned
        let direct_conflict_vars = db.map(conflicting_pairs, |((_, v), _)| *v);
        let direct_conflict_vars_distinct = db.distinct(direct_conflict_vars);

        // Map conflicts to Conflict enum
        let clause_conflict_enums = db.map(clause_conflicts, |cid| Conflict::EmptyClause(*cid));
        let direct_conflict_enums = db.map(direct_conflict_vars_distinct, |v| {
            Conflict::DirectConflict(*v)
        });

        // All conflicts
        let conflicts = db.union(clause_conflict_enums, direct_conflict_enums);

        // === Set up interrupts for early conflict detection ===
        // Interrupt 1: Empty clause (all literals false)
        db.interrupt(clause_conflict_enums);

        // Interrupt 2: Both literal and its negation assigned
        db.interrupt(direct_conflict_enums);

        // === Set up the feedback loop ===
        // Cartesian product of units with current_level (join on unit key)
        // units is (ClauseId, Lit) - we want (Lit, Level, ClauseId)
        let unit_with_level = db.join(units, current_level_rel, |_| (), |_| ());
        let unit_lit_level_cid =
            db.map(unit_with_level, |((cid, lit), level)| (*lit, *level, *cid));

        // Combine with decision_assignments for the base case
        // decision_assignments is (Lit, Level, ClauseId) - already has DECISION as ClauseId
        let all_new_assignments = db.union(decision_assignments, unit_lit_level_cid);

        // Set up the feedback: prep_assignments accumulates all assignments with timestamps
        db.feedback_with_id(prep_var, decision_assignments, all_new_assignments);

        // Initialize with Level::TOP so unit propagation works at level 0
        db.insert(levels, Level::TOP);
        db.commit();

        Solver {
            db,
            clauses,
            learned,
            levels,
            decision_assignments,
            current_level_rel,
            prep_assignments,
            assignments,
            causes,
            assigned,
            units,
            conflicts,
            current_level: Level::TOP,
            next_learned_id: ClauseId::new(1_000_000),
            num_vars,
            decision_stack: Vec::new(),
        }
    }

    /// Add an original clause to the solver.
    pub fn add_clause(&mut self, clause_id: ClauseId, literals: &[Lit]) {
        for &lit in literals {
            self.db.insert(self.clauses, (clause_id, lit));
        }
        self.db.commit();
    }

    /// Make a decision: assign a literal at a new decision level.
    /// `tried_opposite` indicates if we've already tried the opposite polarity.
    pub(super) fn decide_internal(&mut self, lit: Lit, tried_opposite: bool) {
        self.db.push(None);
        self.current_level.inc();
        self.decision_stack
            .push((self.current_level, lit, tried_opposite));

        // Insert the new level and the decision assignment (with ClauseId::DECISION)
        self.db.insert(self.levels, self.current_level);
        self.db.insert(
            self.decision_assignments,
            (lit, self.current_level, ClauseId::DECISION),
        );
        self.db.commit();
        // The feedback loop will automatically propagate units
    }

    /// Make a decision: assign a literal at a new decision level.
    pub fn decide(&mut self, lit: Lit) {
        self.decide_internal(lit, false);
    }

    /// Propagate units until fixpoint or conflict.
    /// Returns Ok(()) if no conflict, Err(conflict) if conflict found.
    ///
    /// With the feedback-based approach, propagation happens automatically
    /// when we commit. This method just checks for conflicts.
    pub fn propagate(&mut self) -> Result<(), Conflict> {
        // The feedback loop has already propagated to fixpoint
        // Just check for conflicts
        let conflicts: Vec<_> = self.db.collect(self.conflicts);
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
            self.db.insert(self.learned, (cid, lit));
        }
        self.db.commit();
        cid
    }

}
