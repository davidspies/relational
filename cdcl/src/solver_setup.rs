//! CDCL Solver dataflow setup and constructor.

use relational::database::{CommitId, Database, output, output_with_sink};
use relational::{assign, assign_saved, create_input, create_persistent_input, create_variable};

use super::solver::{Inputs, Outputs, Solver, State};
use super::types::{ClauseId, Conflict, Level, var};

impl Solver {
    /// Create a new solver for the given number of variables.
    pub fn new(num_vars: super::types::Var) -> Self {
        let mut db = Database::new();

        // === Input Relations ===
        create_input!(db, clauses, clauses_rel, (ClauseId, super::types::Lit));
        create_persistent_input!(db, learned, learned_rel, (ClauseId, super::types::Lit));
        create_input!(db, mut levels, levels_rel, Level);
        create_input!(
            db,
            decision_assignments,
            decision_assignments_rel,
            (super::types::Lit, Level, ClauseId)
        );

        // Current level = max(levels)
        assign!(
            current_level_rel,
            levels_rel
                .max(|_| (), |l| *l)
                .map(|((), level)| level)
                .boxed()
        );

        // All clauses (original + learned)
        assign_saved!(all_clauses, clauses_rel.union(learned_rel).boxed());

        // === Feedback-based Unit Propagation ===
        // prep_assignments accumulates ((Lit, Level, ClauseId), CommitId) via feedback_with_id
        create_variable!(
            db,
            prep_var,
            prep_var_rel,
            ((super::types::Lit, Level, ClauseId), CommitId)
        );
        assign_saved!(prep_rel, prep_var_rel);

        // Final assignments: for each literal, take the entry with minimum CommitId
        assign!(
            assignments_with_id,
            prep_rel
                .get()
                .min(|((lit, _, _), _)| *lit, |((_, level, _), id)| (*level, *id))
        );
        assign_saved!(
            assignments,
            assignments_with_id
                .map(|(lit, (level, _id))| (lit, level))
                .boxed()
        );

        // Causes: tracks all ways each literal was derived
        assign!(
            causes,
            prep_rel
                .get()
                .map(|((lit, level, cid), commit_id)| ((lit, commit_id), (cid, level)))
                .boxed()
        );

        // Derived: which literals are assigned true
        assign_saved!(assigned, assignments.get().map(|(lit, _)| lit).boxed());

        // === Compute Units ===
        assign!(
            clause_lit_true,
            all_clauses
                .get()
                .join(assigned.get(), |(_, lit)| *lit, |lit| *lit)
        );
        assign!(
            satisfied_clauses,
            clause_lit_true.map(|((cid, _), _)| cid).boxed()
        );

        assign!(assigned_vars, assigned.get().map(var).boxed());
        assign!(
            clause_lit_with_var,
            all_clauses
                .get()
                .map(|(cid, lit)| (cid, lit, var(lit)))
                .boxed()
        );
        assign!(
            clause_assigned_lits,
            clause_lit_with_var.join(assigned_vars, |(_, _, v)| *v, |v| *v)
        );
        assign!(
            clause_assigned_lit_ids,
            clause_assigned_lits
                .map(|((cid, lit, _), _)| (cid, lit))
                .boxed()
        );

        assign_saved!(
            clause_unassigned_lits,
            all_clauses
                .get()
                .difference(clause_assigned_lit_ids)
                .boxed()
        );
        assign_saved!(
            unassigned_count,
            clause_unassigned_lits.get().count(|(cid, _)| *cid).boxed()
        );

        assign!(
            unit_candidate_clauses,
            unassigned_count.get().filter(|(_, cnt)| *cnt == 1)
        );
        assign!(
            unit_clause_ids,
            unit_candidate_clauses.map(|(cid, _)| cid).boxed()
        );

        assign!(
            units_with_lit,
            unit_clause_ids.join(clause_unassigned_lits.get(), |cid| *cid, |(cid, _)| *cid)
        );
        assign_saved!(
            potential_units,
            units_with_lit.map(|(cid, (_, lit))| (cid, lit)).boxed()
        );

        assign_saved!(satisfied_set, satisfied_clauses);
        assign!(
            unit_clause_sat_check,
            potential_units
                .get()
                .join(satisfied_set.get(), |(cid, _)| *cid, |cid| *cid)
        );
        assign!(
            units_from_sat,
            unit_clause_sat_check
                .map(|((cid, lit), _)| (cid, lit))
                .boxed()
        );

        assign!(
            units,
            potential_units.get().difference(units_from_sat).boxed()
        );

        // === Conflict Detection ===
        assign!(
            all_clause_ids,
            all_clauses.get().map(|(cid, _)| cid).boxed()
        );
        assign!(all_clause_ids_distinct, all_clause_ids.distinct().boxed());
        assign!(
            clauses_with_unassigned,
            unassigned_count.get().map(|(cid, _)| cid).boxed()
        );
        assign!(
            fully_assigned_clauses,
            all_clause_ids_distinct
                .difference(clauses_with_unassigned)
                .boxed()
        );
        assign!(satisfied_distinct, satisfied_set.get().distinct().boxed());
        assign!(
            clause_conflicts,
            fully_assigned_clauses
                .difference(satisfied_distinct)
                .boxed()
        );

        assign_saved!(
            assigned_with_var,
            assigned.get().map(|lit| (lit, var(lit))).boxed()
        );
        assign!(
            both_polarities,
            assigned_with_var
                .get()
                .join(assigned_with_var.get(), |(_, v)| *v, |(_, v)| *v)
        );
        assign!(
            conflicting_pairs,
            both_polarities.filter(|((lit1, _), (lit2, _))| lit1 != lit2)
        );
        assign!(
            direct_conflict_vars,
            conflicting_pairs.map(|((_, v), _)| v).boxed()
        );
        assign!(
            direct_conflict_vars_distinct,
            direct_conflict_vars.distinct().boxed()
        );

        assign!(
            clause_conflict_enums,
            clause_conflicts.map(Conflict::EmptyClause).boxed()
        );
        assign!(
            direct_conflict_enums,
            direct_conflict_vars_distinct
                .map(Conflict::DirectConflict)
                .boxed()
        );

        assign_saved!(
            conflicts,
            clause_conflict_enums.union(direct_conflict_enums).boxed()
        );

        // === Set up interrupts for early conflict detection ===
        db.interrupt(conflicts.get());

        // === Set up the feedback loop ===
        assign!(
            unit_with_level,
            units.join(current_level_rel, |_| (), |_| ())
        );
        assign!(
            unit_lit_level_cid,
            unit_with_level
                .map(|((cid, lit), level)| (lit, level, cid))
                .boxed()
        );

        assign!(
            all_new_assignments,
            decision_assignments_rel.union(unit_lit_level_cid).boxed()
        );

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
