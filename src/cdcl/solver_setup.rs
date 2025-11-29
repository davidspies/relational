//! CDCL Solver dataflow setup and constructor.

use crate::Database;

use super::solver::Solver;
use super::types::{var, ClauseId, Conflict, Level};

impl Solver {
    /// Create a new solver for the given number of variables.
    pub fn new(num_vars: super::types::Var) -> Self {
        let mut db = Database::new();

        // === Input Relations ===
        let clauses = db.create_input::<(ClauseId, super::types::Lit)>("clauses");
        let learned = db.create_persistent_input::<(ClauseId, super::types::Lit)>("learned");

        // Levels input - we insert decision levels here
        let levels = db.create_input::<Level>("levels");

        // Decision assignments - inserted directly for decisions (with ClauseId::DECISION)
        let decision_assignments =
            db.create_input::<(super::types::Lit, Level, ClauseId)>("decision_assignments");

        // Current level = max(levels)
        let current_level_rel = db.max(levels);

        // All clauses (original + learned)
        let all_clauses = db.union(clauses, learned);

        // === Feedback-based Unit Propagation ===
        // prep_assignments accumulates ((Lit, Level, ClauseId), CommitId) via feedback_with_id
        let (prep_var, prep_assignments) = db.variable::<(
            (super::types::Lit, Level, ClauseId),
            crate::database::CommitId,
        )>("prep_assignments");

        // Final assignments: for each literal, take the entry with minimum CommitId
        let assignments_with_id = db.group_min(
            prep_assignments,
            |((lit, _, _), _)| *lit,
            |((_, level, _), id)| (*level, *id),
        );
        let assignments = db.map(assignments_with_id, |(lit, (level, _id))| (*lit, *level));

        // Causes: tracks all ways each literal was derived
        let causes = db.map(prep_assignments, |((lit, level, cid), commit_id)| {
            ((*lit, *commit_id), (*cid, *level))
        });

        // Derived: which literals are assigned true
        let assigned = db.map(assignments, |(lit, _)| *lit);

        // === Compute Units ===
        let clause_lit_true = db.join(all_clauses, assigned, |(_, lit)| *lit, |lit| *lit);
        let satisfied_clauses = db.map(clause_lit_true, |((cid, _), _)| *cid);

        let assigned_vars = db.map(assigned, |lit| var(*lit));
        let clause_lit_with_var = db.map(all_clauses, |(cid, lit)| (*cid, *lit, var(*lit)));
        let clause_assigned_lits =
            db.join(clause_lit_with_var, assigned_vars, |(_, _, v)| *v, |v| *v);
        let clause_assigned_lit_ids =
            db.map(clause_assigned_lits, |((cid, lit, _), _)| (*cid, *lit));

        let clause_unassigned_lits = db.difference(all_clauses, clause_assigned_lit_ids);
        let unassigned_count = db.group_count(clause_unassigned_lits, |(cid, _)| *cid);

        let unit_candidate_clauses = db.filter(unassigned_count, |(_, count)| *count == 1);
        let unit_clause_ids = db.map(unit_candidate_clauses, |(cid, _)| *cid);

        let units_with_lit = db.join(
            unit_clause_ids,
            clause_unassigned_lits,
            |cid| *cid,
            |(cid, _)| *cid,
        );
        let potential_units = db.map(units_with_lit, |(cid, (_, lit))| (*cid, *lit));

        let satisfied_set = db.map(satisfied_clauses, |cid| *cid);
        let unit_clause_sat_check =
            db.join(potential_units, satisfied_set, |(cid, _)| *cid, |cid| *cid);
        let units_from_sat = db.map(unit_clause_sat_check, |((cid, lit), _)| (*cid, *lit));

        let units = db.difference(potential_units, units_from_sat);

        // === Conflict Detection ===
        let all_clause_ids = db.map(all_clauses, |(cid, _)| *cid);
        let all_clause_ids_distinct = db.distinct(all_clause_ids);
        let clauses_with_unassigned = db.map(unassigned_count, |(cid, _)| *cid);
        let fully_assigned_clauses =
            db.difference(all_clause_ids_distinct, clauses_with_unassigned);
        let satisfied_distinct = db.distinct(satisfied_set);
        let clause_conflicts = db.difference(fully_assigned_clauses, satisfied_distinct);

        let assigned_with_var = db.map(assigned, |lit| (*lit, var(*lit)));
        let both_polarities = db.join(
            assigned_with_var,
            assigned_with_var,
            |(_, v)| *v,
            |(_, v)| *v,
        );
        let conflicting_pairs = db.filter(both_polarities, |((lit1, _), (lit2, _))| lit1 != lit2);
        let direct_conflict_vars = db.map(conflicting_pairs, |((_, v), _)| *v);
        let direct_conflict_vars_distinct = db.distinct(direct_conflict_vars);

        let clause_conflict_enums = db.map(clause_conflicts, |cid| Conflict::EmptyClause(*cid));
        let direct_conflict_enums =
            db.map(direct_conflict_vars_distinct, |v| Conflict::DirectConflict(*v));

        let conflicts = db.union(clause_conflict_enums, direct_conflict_enums);

        // === Set up interrupts for early conflict detection ===
        db.interrupt(clause_conflict_enums);
        db.interrupt(direct_conflict_enums);

        // === Set up the feedback loop ===
        let unit_with_level = db.join(units, current_level_rel, |_| (), |_| ());
        let unit_lit_level_cid =
            db.map(unit_with_level, |((cid, lit), level)| (*lit, *level, *cid));

        let all_new_assignments = db.union(decision_assignments, unit_lit_level_cid);

        db.feedback_with_id(prep_var, all_new_assignments);

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
}
