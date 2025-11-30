//! CDCL Solver dataflow setup and constructor.

use relational::database::{CommitId, Database, output, output_with_sink};

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
        let current_level_rel = levels_rel
            .max(|_| (), |l| *l)
            .map(|((), level)| level)
            .boxed();

        // All clauses (original + learned)
        let all_clauses = clauses_rel.union(learned_rel).boxed().save();

        // === Feedback-based Unit Propagation ===
        // prep_assignments accumulates ((Lit, Level, ClauseId), CommitId) via feedback_with_id
        let (prep_var, prep_var_rel) =
            db.create_variable::<((super::types::Lit, Level, ClauseId), CommitId)>();
        let prep_rel = prep_var_rel.save();

        // Final assignments: for each literal, take the entry with minimum CommitId
        let assignments_with_id = prep_rel
            .get()
            .min(|((lit, _, _), _)| *lit, |((_, level, _), id)| (*level, *id));
        let assignments = assignments_with_id
            .map(|(lit, (level, _id))| (lit, level))
            .boxed()
            .save();

        // Causes: tracks all ways each literal was derived
        let causes = prep_rel
            .get()
            .map(|((lit, level, cid), commit_id)| ((lit, commit_id), (cid, level)))
            .boxed();

        // Derived: which literals are assigned true
        let assigned = assignments.get().map(|(lit, _)| lit).boxed().save();

        // === Compute Units ===
        let clause_lit_true = all_clauses
            .get()
            .join(assigned.get(), |(_, lit)| *lit, |lit| *lit);
        let satisfied_clauses = clause_lit_true.map(|((cid, _), _)| cid).boxed();

        let assigned_vars = assigned.get().map(var).boxed();
        let clause_lit_with_var = all_clauses
            .get()
            .map(|(cid, lit)| (cid, lit, var(lit)))
            .boxed();
        let clause_assigned_lits = clause_lit_with_var.join(assigned_vars, |(_, _, v)| *v, |v| *v);
        let clause_assigned_lit_ids = clause_assigned_lits
            .map(|((cid, lit, _), _)| (cid, lit))
            .boxed();

        let clause_unassigned_lits = all_clauses
            .get()
            .difference(clause_assigned_lit_ids)
            .boxed()
            .save();
        let unassigned_count = clause_unassigned_lits
            .get()
            .count(|(cid, _)| *cid)
            .boxed()
            .save();

        let unit_candidate_clauses = unassigned_count.get().filter(|(_, cnt)| *cnt == 1);
        let unit_clause_ids = unit_candidate_clauses.map(|(cid, _)| cid).boxed();

        let units_with_lit =
            unit_clause_ids.join(clause_unassigned_lits.get(), |cid| *cid, |(cid, _)| *cid);
        let potential_units = units_with_lit
            .map(|(cid, (_, lit))| (cid, lit))
            .boxed()
            .save();

        let satisfied_set = satisfied_clauses.save();
        let unit_clause_sat_check =
            potential_units
                .get()
                .join(satisfied_set.get(), |(cid, _)| *cid, |cid| *cid);
        let units_from_sat = unit_clause_sat_check
            .map(|((cid, lit), _)| (cid, lit))
            .boxed();

        let units = potential_units.get().difference(units_from_sat).boxed();

        // === Conflict Detection ===
        let all_clause_ids = all_clauses.get().map(|(cid, _)| cid).boxed();
        let all_clause_ids_distinct = all_clause_ids.distinct().boxed();
        let clauses_with_unassigned = unassigned_count.get().map(|(cid, _)| cid).boxed();
        let fully_assigned_clauses = all_clause_ids_distinct
            .difference(clauses_with_unassigned)
            .boxed();
        let satisfied_distinct = satisfied_set.get().distinct().boxed();
        let clause_conflicts = fully_assigned_clauses
            .difference(satisfied_distinct)
            .boxed();

        let assigned_with_var = assigned.get().map(|lit| (lit, var(lit))).boxed().save();
        let both_polarities =
            assigned_with_var
                .get()
                .join(assigned_with_var.get(), |(_, v)| *v, |(_, v)| *v);
        let conflicting_pairs = both_polarities.filter(|((lit1, _), (lit2, _))| lit1 != lit2);
        let direct_conflict_vars = conflicting_pairs.map(|((_, v), _)| v).boxed();
        let direct_conflict_vars_distinct = direct_conflict_vars.distinct().boxed();

        let clause_conflict_enums = clause_conflicts.map(Conflict::EmptyClause).boxed();
        let direct_conflict_enums = direct_conflict_vars_distinct
            .map(Conflict::DirectConflict)
            .boxed();

        let conflicts = clause_conflict_enums
            .union(direct_conflict_enums)
            .boxed()
            .save();

        // === Set up interrupts for early conflict detection ===
        db.interrupt(conflicts.get());

        // === Set up the feedback loop ===
        let unit_with_level = units.join(current_level_rel, |_| (), |_| ());
        let unit_lit_level_cid = unit_with_level
            .map(|((cid, lit), level)| (lit, level, cid))
            .boxed();

        let all_new_assignments = decision_assignments_rel.union(unit_lit_level_cid).boxed();

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
