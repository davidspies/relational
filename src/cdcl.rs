//! CDCL SAT Solver built on the relational query engine.
//!
//! This implements Conflict-Driven Clause Learning using:
//! - Relations for clauses, assignments, and implications
//! - Feedback loops for unit propagation (fixpoint)
//! - Push/pop checkpoints for backtracking
//! - Persistent inputs for learned clauses

use std::fmt;

use crate::database::CommitId;
use crate::relation::Relation;
use crate::Database;

/// A literal is a variable with a sign (positive or negative).
/// Positive values represent the variable, negative values represent its negation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Lit(i32);

impl Lit {
    /// Create a positive literal for a variable.
    pub fn pos(v: Var) -> Self {
        Lit(v.0 as i32)
    }

    /// Create a negative literal for a variable.
    pub fn neg(v: Var) -> Self {
        Lit(-(v.0 as i32))
    }

    /// Create a literal from a raw i32 (positive = positive literal, negative = negative literal).
    pub fn from_raw(raw: i32) -> Self {
        assert!(raw != 0, "Literal cannot be 0");
        Lit(raw)
    }

    /// Get the variable this literal refers to.
    pub fn var(self) -> Var {
        Var(self.0.unsigned_abs())
    }

    /// Check if this is a positive literal.
    pub fn is_positive(self) -> bool {
        self.0 > 0
    }

    /// Get the negation of this literal.
    pub fn negated(self) -> Self {
        Lit(-self.0)
    }

    /// Get the raw i32 value.
    pub fn raw(self) -> i32 {
        self.0
    }
}

/// Helper function to get the variable of a literal.
fn var(lit: Lit) -> Var {
    lit.var()
}

/// Helper function to negate a literal.
fn neg(lit: Lit) -> Lit {
    lit.negated()
}

impl fmt::Display for Lit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 > 0 {
            write!(f, "x{}", self.0)
        } else {
            write!(f, "¬x{}", -self.0)
        }
    }
}

/// A variable identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Var(u32);

impl Var {
    /// Create a variable from a 1-indexed number.
    pub fn new(n: u32) -> Self {
        assert!(n > 0, "Variables are 1-indexed");
        Var(n)
    }

    /// Get the raw u32 value.
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for Var {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "x{}", self.0)
    }
}

/// Decision level (0 = top-level/forced, 1+ = decision levels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Level(u32);

impl Level {
    /// The top level (level 0) where unit clauses propagate.
    pub const TOP: Level = Level(0);

    /// Create a new level.
    pub fn new(n: u32) -> Self {
        Level(n)
    }

    /// Get the raw u32 value.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Increment the level.
    pub fn inc(&mut self) {
        self.0 += 1;
    }

    /// Decrement the level.
    pub fn dec(&mut self) {
        self.0 = self.0.saturating_sub(1);
    }
}

/// A clause ID for tracking which clause caused an implication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ClauseId(u32);

impl ClauseId {
    /// A special clause ID indicating a decision (no reason clause).
    pub const DECISION: ClauseId = ClauseId(0);

    /// Create a new clause ID.
    pub fn new(n: u32) -> Self {
        ClauseId(n)
    }

    /// Get the raw u32 value.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Check if this is a decision (no reason clause).
    pub fn is_decision(self) -> bool {
        self.0 == 0
    }
}

/// A conflict detected during propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Conflict {
    /// A clause has all its literals assigned false.
    EmptyClause(ClauseId),
    /// Both a literal and its negation are assigned.
    /// The variable is stored (the literal that was assigned both ways).
    DirectConflict(Var),
}

/// CDCL SAT Solver.
pub struct Solver {
    db: Database,

    // === Input Relations ===
    /// Original clauses: (clause_id, literal)
    /// Each clause is represented as multiple tuples, one per literal.
    clauses: Relation<(ClauseId, Lit)>,

    /// Learned clauses (persistent - survive backtracking)
    learned: Relation<(ClauseId, Lit)>,

    /// Decision levels - we insert the current level here
    levels: Relation<Level>,

    /// Decision assignments (lit, level, clause_id) - inserted directly for decisions
    decision_assignments: Relation<(Lit, Level, ClauseId)>,

    // === Derived/State Relations ===
    /// Current level = max(levels)
    /// Note: Stored to keep the relation alive, accessed via the graph
    #[allow(dead_code)]
    current_level_rel: Relation<Level>,

    /// Prep assignments from feedback_with_id: ((lit, level, clause_id), commit_id)
    /// This accumulates all discovered assignments with their discovery time and reason clause
    /// Note: Stored to keep the relation alive, accessed via the graph
    #[allow(dead_code)]
    prep_assignments: Relation<((Lit, Level, ClauseId), CommitId)>,

    /// Final assignments: (lit, level) - derived by taking min commit_id per lit
    assignments: Relation<(Lit, Level)>,

    /// Causes: ((lit, commit_id), (clause_id, level)) - tracks all ways each literal was derived
    /// Can be collected to HashMap<Lit, BTreeMap<CommitId, Multiset<(ClauseId, Level)>>>
    causes: Relation<((Lit, CommitId), (ClauseId, Level))>,

    /// The "assigned" relation - just tracks which literals are assigned true
    assigned: Relation<Lit>,

    /// Unit clauses that need propagation: (clause_id, implied_literal)
    units: Relation<(ClauseId, Lit)>,

    /// Conflicts detected during propagation
    conflicts: Relation<Conflict>,

    // === Solver State ===
    /// Current decision level (local copy for convenience).
    current_level: Level,

    /// Next clause ID for learned clauses.
    next_learned_id: ClauseId,

    /// Number of variables.
    num_vars: Var,

    /// Stack of decisions: (level, literal, tried_both)
    /// tried_both = true means we've already tried the opposite polarity
    decision_stack: Vec<(Level, Lit, bool)>,
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
        let decision_assignments = db.create_input::<(Lit, Level, ClauseId)>("decision_assignments");

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
            |((lit, _, _), _)| *lit,                    // group by lit
            |((_, level, _), id)| (*level, *id),        // value is (level, commit_id)
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
        let clause_lit_true = db.join(
            all_clauses,
            assigned,
            |(_, lit)| *lit,
            |lit| *lit,
        );
        let satisfied_clauses = db.map(clause_lit_true, |((cid, _), _)| *cid);

        // Literals that are assigned (either true or false)
        let assigned_vars = db.map(assigned, |lit| var(*lit));

        // For each clause literal, check if its variable is assigned
        let clause_lit_with_var = db.map(all_clauses, |(cid, lit)| (*cid, *lit, var(*lit)));
        let clause_assigned_lits = db.join(
            clause_lit_with_var,
            assigned_vars,
            |(_, _, v)| *v,
            |v| *v,
        );
        let clause_assigned_lit_ids = db.map(clause_assigned_lits, |((cid, lit, _), _)| (*cid, *lit));

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
        let unit_clause_sat_check = db.join(
            potential_units,
            satisfied_set,
            |(cid, _)| *cid,
            |cid| *cid,
        );
        let units_from_sat = db.map(unit_clause_sat_check, |((cid, lit), _)| (*cid, *lit));

        // Units = potential_units where clause is NOT satisfied
        let units = db.difference(potential_units, units_from_sat);

        // === Conflict Detection ===
        // Type 1: Clause conflicts (all literals false)
        let all_clause_ids = db.map(all_clauses, |(cid, _)| *cid);
        let all_clause_ids_distinct = db.distinct(all_clause_ids);
        let clauses_with_unassigned = db.map(unassigned_count, |(cid, _)| *cid);
        let fully_assigned_clauses = db.difference(all_clause_ids_distinct, clauses_with_unassigned);
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
        let direct_conflict_enums = db.map(direct_conflict_vars_distinct, |v| Conflict::DirectConflict(*v));

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
        let unit_lit_level_cid = db.map(unit_with_level, |((cid, lit), level)| (*lit, *level, *cid));

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
    fn decide_internal(&mut self, lit: Lit, tried_opposite: bool) {
        self.db.push(None);
        self.current_level.inc();
        self.decision_stack.push((self.current_level, lit, tried_opposite));

        // Insert the new level and the decision assignment (with ClauseId::DECISION)
        self.db.insert(self.levels, self.current_level);
        self.db.insert(self.decision_assignments, (lit, self.current_level, ClauseId::DECISION));
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

    /// Get all currently assigned literals.
    pub fn get_assignments(&self) -> Vec<(Lit, Level)> {
        self.db.collect(self.assignments)
    }

    /// Get the current decision level.
    pub fn level(&self) -> Level {
        self.current_level
    }

    /// Check if a variable is assigned.
    pub fn is_assigned(&self, v: Var) -> bool {
        let assigned: std::collections::HashSet<_> = self.db.collect(self.assigned).into_iter().collect();
        assigned.contains(&Lit::pos(v)) || assigned.contains(&Lit::neg(v))
    }

    /// Get the truth value of a variable, if assigned.
    pub fn value(&self, v: Var) -> Option<bool> {
        let assigned: std::collections::HashSet<_> = self.db.collect(self.assigned).into_iter().collect();
        if assigned.contains(&Lit::pos(v)) {
            Some(true)
        } else if assigned.contains(&Lit::neg(v)) {
            Some(false)
        } else {
            None
        }
    }

    /// Get the next unassigned variable (simple heuristic: lowest numbered).
    pub fn pick_branching_variable(&self) -> Option<Var> {
        for v in 1..=self.num_vars.raw() {
            let var = Var::new(v);
            if !self.is_assigned(var) {
                return Some(var);
            }
        }
        None
    }

    /// Get current conflicts (for debugging).
    pub fn get_conflicts(&self) -> Vec<Conflict> {
        self.db.collect(self.conflicts)
    }

    /// Get current units (for debugging).
    pub fn get_units(&self) -> Vec<(ClauseId, Lit)> {
        self.db.collect(self.units)
    }

    /// Get the causes (implication graph) as a structured data type.
    /// Returns HashMap<Lit, BTreeMap<CommitId, Vec<(ClauseId, Level)>>>
    /// For each literal, this maps each CommitId to the list of (ClauseId, Level) that derived it at that commit.
    /// The Vec acts as a multiset (there can be duplicates if the same clause/level appears multiple times).
    pub fn get_causes(&self) -> std::collections::HashMap<Lit, std::collections::BTreeMap<CommitId, Vec<(ClauseId, Level)>>> {
        use std::collections::{HashMap, BTreeMap};

        let raw: Vec<((Lit, CommitId), (ClauseId, Level))> = self.db.collect(self.causes);
        let mut result: HashMap<Lit, BTreeMap<CommitId, Vec<(ClauseId, Level)>>> = HashMap::new();

        for ((lit, commit_id), (clause_id, level)) in raw {
            result
                .entry(lit)
                .or_default()
                .entry(commit_id)
                .or_default()
                .push((clause_id, level));
        }

        result
    }

    /// Main solve loop.
    pub fn solve(&mut self) -> bool {
        loop {
            // Propagate
            match self.propagate() {
                Ok(()) => {
                    // No conflict - pick next variable or return SAT
                    match self.pick_branching_variable() {
                        Some(v) => {
                            // Decide: try positive literal first
                            self.decide(Lit::pos(v));
                        }
                        None => {
                            // All variables assigned, no conflict = SAT
                            return true;
                        }
                    }
                }
                Err(_conflict_clause) => {
                    // Conflict! Need to backtrack.
                    // Find a decision level where we haven't tried both polarities.
                    loop {
                        if self.current_level == Level::TOP {
                            // Conflict at level 0 = UNSAT
                            return false;
                        }

                        // Get the decision at current level
                        let (_, decision_lit, tried_both) = self.decision_stack.last().copied().unwrap();

                        // Calculate previous level
                        let prev_level = Level::new(self.current_level.raw().saturating_sub(1));

                        if tried_both {
                            // Already tried both polarities at this level, backtrack further
                            self.backtrack_to(prev_level);
                        } else {
                            // Haven't tried opposite polarity yet
                            // Backtrack this level and try the opposite
                            self.backtrack_to(prev_level);
                            self.decide_internal(neg(decision_lit), true);
                            break;
                        }
                    }
                }
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create literals from raw i32
    fn lit(raw: i32) -> Lit {
        Lit::from_raw(raw)
    }

    // Helper to create clause IDs
    fn cid(n: u32) -> ClauseId {
        ClauseId::new(n)
    }

    #[test]
    fn test_simple_sat() {
        // (x1 OR x2) AND (x1 OR NOT x2)
        // SAT: x1 = true
        let mut solver = Solver::new(Var::new(2));
        solver.add_clause(cid(1), &[lit(1), lit(2)]);   // x1 OR x2
        solver.add_clause(cid(2), &[lit(1), lit(-2)]);  // x1 OR NOT x2

        assert!(solver.solve());
        assert_eq!(solver.value(Var::new(1)), Some(true));
    }

    #[test]
    fn test_simple_unsat() {
        // (x1) AND (NOT x1)
        // UNSAT
        let mut solver = Solver::new(Var::new(1));
        solver.add_clause(cid(1), &[lit(1)]);   // x1
        solver.add_clause(cid(2), &[lit(-1)]);  // NOT x1

        assert!(!solver.solve());
    }

    #[test]
    fn test_unit_propagation() {
        // (x1) AND (NOT x1 OR x2) AND (NOT x2 OR x3)
        // Unit prop: x1=T -> x2=T -> x3=T
        let mut solver = Solver::new(Var::new(3));
        solver.add_clause(cid(1), &[lit(1)]);              // x1
        solver.add_clause(cid(2), &[lit(-1), lit(2)]);     // NOT x1 OR x2
        solver.add_clause(cid(3), &[lit(-2), lit(3)]);     // NOT x2 OR x3

        assert!(solver.solve());
        assert_eq!(solver.value(Var::new(1)), Some(true));
        assert_eq!(solver.value(Var::new(2)), Some(true));
        assert_eq!(solver.value(Var::new(3)), Some(true));
    }

    #[test]
    fn test_backtracking() {
        // (x1 OR x2) AND (NOT x1 OR x2) AND (x1 OR NOT x2) AND (NOT x1 OR NOT x2)
        // This is UNSAT (pigeon hole for 2 pigeons, 1 hole)
        let mut solver = Solver::new(Var::new(2));
        solver.add_clause(cid(1), &[lit(1), lit(2)]);      // x1 OR x2
        solver.add_clause(cid(2), &[lit(-1), lit(2)]);     // NOT x1 OR x2
        solver.add_clause(cid(3), &[lit(1), lit(-2)]);     // x1 OR NOT x2
        solver.add_clause(cid(4), &[lit(-1), lit(-2)]);    // NOT x1 OR NOT x2

        assert!(!solver.solve());
    }
}
