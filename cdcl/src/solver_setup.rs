//! CDCL Solver dataflow setup and constructor.

use relational::database::{
    CommitId, Database, Op, count, difference, distinct, filter, join, map, max, min, output,
    output_with_sink, union,
};

use super::solver::{Inputs, Outputs, Solver, State};
use super::types::{ClauseId, Conflict, Level, var};

impl Solver {
    /// Create a new solver for the given number of variables.
    pub fn new(num_vars: super::types::Var) -> Self {
        let mut db = Database::new();

        // === Input Relations ===
        let (clauses, clauses_rel) = db.create_input::<(ClauseId, super::types::Lit)>();
        let (learned, learned_rel) = db.create_persistent_input::<(ClauseId, super::types::Lit)>();

        // Levels input - we insert decision levels here
        let (mut levels, levels_rel) = db.create_input::<Level>();

        // Decision assignments - inserted directly for decisions (with ClauseId::DECISION)
        let (decision_assignments, decision_assignments_rel) =
            db.create_input::<(super::types::Lit, Level, ClauseId)>();

        // Current level = max(levels)
        let current_level_rel = max(levels_rel, |_| (), |l| *l);
        let current_level_rel = map(current_level_rel, |((), level)| level).boxed();

        // All clauses (original + learned)
        let all_clauses = db.save(union(clauses_rel, learned_rel).boxed());

        // === Feedback-based Unit Propagation ===
        // prep_assignments accumulates ((Lit, Level, ClauseId), CommitId) via feedback_with_id
        let (prep_var, prep_var_rel) =
            db.create_variable::<((super::types::Lit, Level, ClauseId), CommitId)>();
        let prep_rel = db.save(prep_var_rel);

        // Final assignments: for each literal, take the entry with minimum CommitId
        let assignments_with_id = min(
            prep_rel.get(),
            |((lit, _, _), _)| *lit,
            |((_, level, _), id)| (*level, *id),
        );
        let assignments =
            db.save(map(assignments_with_id, |(lit, (level, _id))| (lit, level)).boxed());

        // Causes: tracks all ways each literal was derived
        let causes = map(prep_rel.get(), |((lit, level, cid), commit_id)| {
            ((lit, commit_id), (cid, level))
        })
        .boxed();

        // Derived: which literals are assigned true
        let assigned = db.save(map(assignments.get(), |(lit, _)| lit).boxed());

        // === Compute Units ===
        let clause_lit_true = join(
            all_clauses.get(),
            assigned.get(),
            |(_, lit)| *lit,
            |lit| *lit,
        );
        let satisfied_clauses = map(clause_lit_true, |((cid, _), _)| cid).boxed();

        let assigned_vars = map(assigned.get(), var).boxed();
        let clause_lit_with_var = map(all_clauses.get(), |(cid, lit)| (cid, lit, var(lit))).boxed();
        let clause_assigned_lits = join(clause_lit_with_var, assigned_vars, |(_, _, v)| *v, |v| *v);
        let clause_assigned_lit_ids =
            map(clause_assigned_lits, |((cid, lit, _), _)| (cid, lit)).boxed();

        let clause_unassigned_lits =
            db.save(difference(all_clauses.get(), clause_assigned_lit_ids).boxed());
        let unassigned_count =
            db.save(count(clause_unassigned_lits.get(), |(cid, _)| *cid).boxed());

        let unit_candidate_clauses = filter(unassigned_count.get(), |(_, cnt)| *cnt == 1);
        let unit_clause_ids = map(unit_candidate_clauses, |(cid, _)| cid).boxed();

        let units_with_lit = join(
            unit_clause_ids,
            clause_unassigned_lits.get(),
            |cid| *cid,
            |(cid, _)| *cid,
        );
        let potential_units = db.save(map(units_with_lit, |(cid, (_, lit))| (cid, lit)).boxed());

        let satisfied_set = db.save(satisfied_clauses);
        let unit_clause_sat_check = join(
            potential_units.get(),
            satisfied_set.get(),
            |(cid, _)| *cid,
            |cid| *cid,
        );
        let units_from_sat = map(unit_clause_sat_check, |((cid, lit), _)| (cid, lit)).boxed();

        let units = difference(potential_units.get(), units_from_sat).boxed();

        // === Conflict Detection ===
        let all_clause_ids = map(all_clauses.get(), |(cid, _)| cid).boxed();
        let all_clause_ids_distinct = distinct(all_clause_ids).boxed();
        let clauses_with_unassigned = map(unassigned_count.get(), |(cid, _)| cid).boxed();
        let fully_assigned_clauses =
            difference(all_clause_ids_distinct, clauses_with_unassigned).boxed();
        let satisfied_distinct = distinct(satisfied_set.get()).boxed();
        let clause_conflicts = difference(fully_assigned_clauses, satisfied_distinct).boxed();

        let assigned_with_var = db.save(map(assigned.get(), |lit| (lit, var(lit))).boxed());
        let both_polarities = join(
            assigned_with_var.get(),
            assigned_with_var.get(),
            |(_, v)| *v,
            |(_, v)| *v,
        );
        let conflicting_pairs = filter(both_polarities, |((lit1, _), (lit2, _))| lit1 != lit2);
        let direct_conflict_vars = map(conflicting_pairs, |((_, v), _)| v).boxed();
        let direct_conflict_vars_distinct = distinct(direct_conflict_vars).boxed();

        let clause_conflict_enums = map(clause_conflicts, Conflict::EmptyClause).boxed();
        let direct_conflict_enums = map(direct_conflict_vars_distinct, |v| {
            Conflict::DirectConflict(v)
        })
        .boxed();

        let conflicts = db.save(union(clause_conflict_enums, direct_conflict_enums).boxed());

        // === Set up interrupts for early conflict detection ===
        db.interrupt(conflicts.get());

        // === Set up the feedback loop ===
        let unit_with_level = join(units, current_level_rel, |_| (), |_| ());
        let unit_lit_level_cid =
            map(unit_with_level, |((cid, lit), level)| (lit, level, cid)).boxed();

        let all_new_assignments = union(decision_assignments_rel, unit_lit_level_cid).boxed();

        db.feedback_with_id(prep_var, all_new_assignments);

        // Initialize with Level::TOP so unit propagation works at level 0
        levels.insert(Level::TOP);
        db.commit();

        // Create outputs from relations (need to box them to store in struct)
        let assignments_out = output_with_sink(assignments.get().boxed());
        let causes_out = output_with_sink(causes.boxed());
        let assigned_out = output(assigned.get().boxed());
        let conflicts_out = output(conflicts.get().boxed());

        Solver {
            db,
            inputs: Inputs {
                clauses,
                learned,
                levels,
                decision_assignments,
            },
            outputs: Outputs {
                assignments: assignments_out,
                causes: causes_out,
                assigned: assigned_out,
                conflicts: conflicts_out,
            },
            state: State {
                current_level: Level::TOP,
                next_learned_id: ClauseId::new(1),
                num_vars,
                decision_stack: Vec::new(),
                clause_db: std::collections::HashMap::new(),
            },
        }
    }
}
