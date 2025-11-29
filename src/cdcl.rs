//! CDCL SAT Solver built on the relational query engine.
//!
//! This implements Conflict-Driven Clause Learning using:
//! - Relations for clauses, assignments, and implications
//! - Feedback loops for unit propagation (fixpoint)
//! - Push/pop checkpoints for backtracking
//! - Persistent inputs for learned clauses

use std::fmt;

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

/// CDCL SAT Solver.
pub struct Solver {
    db: Database,

    // === Input Relations ===
    /// Original clauses: (clause_id, literal)
    /// Each clause is represented as multiple tuples, one per literal.
    clauses: Relation<(ClauseId, Lit)>,

    /// Learned clauses (persistent - survive backtracking)
    learned: Relation<(ClauseId, Lit)>,

    // === Derived/State Relations ===
    /// Current assignments: (literal, level, reason_clause)
    /// reason_clause = 0 means decision, otherwise it's the clause that implied it
    assignments: Relation<(Lit, Level, ClauseId)>,

    /// The "assigned" relation - just tracks which literals are assigned true
    assigned: Relation<Lit>,

    /// Unit clauses that need propagation: (clause_id, implied_literal)
    units: Relation<(ClauseId, Lit)>,

    /// Conflicts: clause_id of conflicting clauses
    conflicts: Relation<ClauseId>,

    // === Solver State ===
    /// Current decision level.
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

        // Input relations
        let clauses = db.create_input::<(ClauseId, Lit)>("clauses");
        let learned = db.create_persistent_input::<(ClauseId, Lit)>("learned");

        // State relation for assignments
        let assignments = db.create_input::<(Lit, Level, ClauseId)>("assignments");

        // Derived: which literals are assigned true
        let assigned = db.map(assignments, |(lit, _, _)| *lit);

        // All clauses (original + learned)
        let all_clauses = db.union(clauses, learned);

        // === Unit Propagation ===
        // A clause is unit when:
        // - It has exactly one unassigned literal
        // - All other literals are assigned false (equivalently: 0 true literals)
        //
        // Since we're doing SAT, a literal being "true" means it's in `assigned`.
        // A clause is satisfied if any literal in it is true.
        // A clause is unit if: exactly 1 unassigned literal AND 0 true literals.

        // Literals that are true (in assigned)
        // For each (clause_id, lit), check if lit is true (satisfied)
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

        // Unassigned literals in clauses = all_clauses - clause_assigned_lit_ids
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

        // Filter out satisfied clauses - only propagate from unsatisfied unit clauses
        let satisfied_set = db.map(satisfied_clauses, |cid| *cid);
        let unit_clause_sat_check = db.join(
            potential_units,
            satisfied_set,
            |(cid, _)| *cid,
            |cid| *cid,
        );
        let _satisfied_unit_clauses = db.map(unit_clause_sat_check, |((cid, _), _)| *cid);
        let units_from_sat = db.map(unit_clause_sat_check, |((cid, lit), _)| (*cid, *lit));

        // Units = potential_units where clause is NOT satisfied
        let units = db.difference(potential_units, units_from_sat);

        // === Conflict Detection ===
        // A clause is in conflict when ALL its literals are assigned false
        // Equivalently: 0 unassigned literals AND 0 true literals (not satisfied)
        //
        // Clauses with 0 unassigned literals: total_count - unassigned_count = total_count
        // Actually, easier: clauses NOT in unassigned_count (no unassigned literals)

        // Get all clause IDs
        let all_clause_ids = db.map(all_clauses, |(cid, _)| *cid);
        let all_clause_ids_distinct = db.distinct(all_clause_ids);

        // Clauses with unassigned literals
        let clauses_with_unassigned = db.map(unassigned_count, |(cid, _)| *cid);

        // Clauses with NO unassigned literals = all - clauses_with_unassigned
        let fully_assigned_clauses = db.difference(all_clause_ids_distinct, clauses_with_unassigned);

        // Conflict = fully assigned AND not satisfied
        let satisfied_distinct = db.distinct(satisfied_set);
        let conflicts = db.difference(fully_assigned_clauses, satisfied_distinct);

        Solver {
            db,
            clauses,
            learned,
            assignments,
            assigned,
            units,
            conflicts,
            current_level: Level::TOP,
            next_learned_id: ClauseId::new(1_000_000), // Start learned clause IDs high to avoid collision
            num_vars,
            decision_stack: Vec::new(),
        }
    }

    /// Add an original clause to the solver.
    pub fn add_clause(&mut self, clause_id: ClauseId, literals: &[Lit]) {
        for &lit in literals {
            self.db.insert(self.clauses, (clause_id, lit));
        }
    }

    /// Make a decision: assign a literal at a new decision level.
    /// `tried_opposite` indicates if we've already tried the opposite polarity.
    fn decide_internal(&mut self, lit: Lit, tried_opposite: bool) {
        self.db.push(None);
        self.current_level.inc();
        self.decision_stack.push((self.current_level, lit, tried_opposite));
        self.db.insert(self.assignments, (lit, self.current_level, ClauseId::DECISION));
    }

    /// Make a decision: assign a literal at a new decision level.
    pub fn decide(&mut self, lit: Lit) {
        self.decide_internal(lit, false);
    }

    /// Propagate units until fixpoint or conflict.
    /// Returns Ok(()) if no conflict, Err(clause_id) if conflict found.
    pub fn propagate(&mut self) -> Result<(), ClauseId> {
        loop {
            // Check for conflicts first
            let conflicts: Vec<_> = self.db.collect(self.conflicts);
            if let Some(&cid) = conflicts.first() {
                return Err(cid);
            }

            // Get units to propagate
            let units: Vec<_> = self.db.collect(self.units);

            // Filter out already assigned literals
            let assigned: std::collections::HashSet<_> = self.db.collect(self.assigned).into_iter().collect();

            // Check for contradictory units first
            for (reason, lit) in &units {
                if assigned.contains(&neg(*lit)) {
                    // The negation is already assigned, this is a conflict!
                    return Err(*reason);
                }
            }

            // Filter to only truly new units
            let new_units: Vec<_> = units
                .into_iter()
                .filter(|(_, lit)| !assigned.contains(lit))
                .collect();

            if new_units.is_empty() {
                return Ok(());
            }

            // Propagate one unit at a time to catch conflicts properly
            // (If we propagate multiple at once, we might miss detecting when
            // two units in the same batch contradict each other)
            let (reason, lit) = new_units[0];
            self.db.insert(self.assignments, (lit, self.current_level, reason));
        }
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
        cid
    }

    /// Get all currently assigned literals.
    pub fn get_assignments(&self) -> Vec<(Lit, Level, ClauseId)> {
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
    pub fn get_conflicts(&self) -> Vec<ClauseId> {
        self.db.collect(self.conflicts)
    }

    /// Get current units (for debugging).
    pub fn get_units(&self) -> Vec<(ClauseId, Lit)> {
        self.db.collect(self.units)
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
