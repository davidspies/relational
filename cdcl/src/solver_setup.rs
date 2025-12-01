//! CDCL Solver dataflow setup and constructor.

use relational::database::{CommitId, Database, output, output_with_sink};
use relational::{assign, assign_saved, create_input, create_persistent_input, create_variable};

use super::solver::{Inputs, Outputs, Solver, State};
use super::types::{ClauseId, Conflict, Level, Lit, var};

impl Solver {
    /// Create a new solver for the given number of variables.
    pub fn new(num_vars: super::types::Var) -> Self {
        let mut db = Database::new();

        // === Input Relations ===
        create_input!(db, clauses, clauses_rel, (ClauseId, Lit));
        create_persistent_input!(db, learned, learned_rel, (ClauseId, Lit));
        create_input!(db, mut levels, levels_rel, Level);
        create_input!(
            db,
            decision_assignments,
            decision_assignments_rel,
            (Lit, Level, ClauseId)
        );

        // Current level = max(levels)
        assign!(current_level_rel, levels_rel.global_max());

        // All clauses (original + learned)
        assign_saved!(all_clauses, clauses_rel.union(learned_rel));

        // === Feedback-based Unit Propagation ===
        // prep_assignments accumulates ((Lit, Level, ClauseId), CommitId) via feedback_with_id
        create_variable!(
            db,
            prep_var,
            prep_var_rel,
            ((Lit, Level, ClauseId), CommitId)
        );
        assign_saved!(prep_rel, prep_var_rel);

        // Final assignments: for each literal, take the entry with minimum CommitId
        assign_saved!(
            assignments,
            prep_rel
                .get()
                .map(|((lit, level, _cid), _id)| (lit, level))
                .group_min()
        );

        // Causes: tracks all ways each literal was derived
        assign!(
            causes,
            prep_rel
                .get()
                .map(|((lit, level, cid), commit_id)| ((lit, commit_id), (cid, level)))
        );

        // Derived: which literals are assigned true
        assign_saved!(assigned, assignments.get().fst());

        // === Compute Units ===
        // Clauses with at least one true literal are satisfied
        assign!(
            satisfied_clauses,
            all_clauses.get().swap().semijoin(assigned.get()).snd()
        );

        assign!(assigned_vars, assigned.get().map(var));
        assign!(
            clause_lit_with_var,
            all_clauses.get().map(|(cid, lit)| (cid, lit, var(lit)))
        );
        // Clause-literal pairs where the variable is assigned
        assign!(
            clause_assigned_lit_ids,
            clause_lit_with_var
                .map(|(cid, lit, v)| (v, (cid, lit)))
                .semijoin(assigned_vars)
                .snd()
        );

        assign_saved!(
            clause_unassigned_lits,
            all_clauses.get().difference(clause_assigned_lit_ids)
        );
        assign_saved!(unassigned_count, clause_unassigned_lits.get().group_count());

        assign!(
            unit_candidate_clauses,
            unassigned_count.get().filter(|(_, cnt)| *cnt == 1)
        );
        assign!(unit_clause_ids, unit_candidate_clauses.fst());

        // Get the unassigned literal for each unit clause
        assign_saved!(
            potential_units,
            clause_unassigned_lits.get().semijoin(unit_clause_ids)
        );

        assign_saved!(satisfied_set, satisfied_clauses);
        // Filter out potential units whose clause is already satisfied
        assign!(
            units_from_sat,
            potential_units.get().semijoin(satisfied_set.get())
        );

        assign!(units, potential_units.get().difference(units_from_sat));

        // === Conflict Detection ===
        assign!(all_clause_ids, all_clauses.get().fst());
        assign!(all_clause_ids_distinct, all_clause_ids.distinct());
        assign!(clauses_with_unassigned, unassigned_count.get().fst());
        assign!(
            fully_assigned_clauses,
            all_clause_ids_distinct.difference(clauses_with_unassigned)
        );
        assign!(satisfied_distinct, satisfied_set.get().distinct());
        assign!(
            clause_conflicts,
            fully_assigned_clauses.difference(satisfied_distinct)
        );

        assign_saved!(assigned_with_var, assigned.get().map(|lit| (var(lit), lit)));
        // Self-join to find pairs of literals with the same variable
        assign!(
            both_polarities,
            assigned_with_var.get().join(assigned_with_var.get())
        );
        // Conflict if two different literals have the same variable
        assign!(
            direct_conflict_vars,
            both_polarities
                .filter(|(_, (lit1, lit2))| lit1 != lit2)
                .fst()
        );
        assign!(
            direct_conflict_vars_distinct,
            direct_conflict_vars.distinct()
        );

        assign!(
            clause_conflict_enums,
            clause_conflicts.map(Conflict::EmptyClause)
        );
        assign!(
            direct_conflict_enums,
            direct_conflict_vars_distinct.map(Conflict::DirectConflict)
        );

        assign_saved!(
            conflicts,
            clause_conflict_enums.union(direct_conflict_enums)
        );

        // === Set up interrupts for early conflict detection ===
        db.interrupt(conflicts.get());

        // === Set up the feedback loop ===
        assign!(unit_with_level, units.cartesian_product(current_level_rel));
        assign!(
            unit_lit_level_cid,
            unit_with_level.map(|((cid, lit), level)| (lit, level, cid))
        );

        assign!(
            all_new_assignments,
            decision_assignments_rel.union(unit_lit_level_cid)
        );

        db.feedback_with_id(prep_var, all_new_assignments);

        // Initialize with Level::TOP so unit propagation works at level 0
        levels.insert(Level::TOP);
        db.commit();

        // Create outputs from relations (need to box them to store in struct)
        let assignments_out = output_with_sink(assignments.get());
        let causes_out = output_with_sink(causes);
        let assigned_out = output(assigned.get());
        let conflicts_out = output(conflicts.get());

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
