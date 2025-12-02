//! CDCL Solver dataflow setup and constructor.

use std::collections::HashMap;
use std::ops::Not;

use relational::database::{CommitId, DatabaseBuilder};
use relational::{assign, assign_saved, create_input, create_persistent_input, create_variable};

use crate::Conflict;
use crate::types::var;

use super::solver::{Inputs, Outputs, Solver, State};
use super::types::{ClauseId, Level, Lit};

impl Solver {
    /// Create a new solver using the provided database builder.
    ///
    /// The caller is responsible for calling `db.build()` after this returns
    /// and passing the resulting `&mut Database` to solver methods.
    pub fn new(db: &mut DatabaseBuilder) -> Self {
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

        assign!(
            all_clause_ids,
            all_clauses.get().fst().consolidate().distinct()
        );

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
                .consolidate()
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

        // === Conflict Detection ===
        // Direct conflict: both a literal and its negation are assigned
        assign_saved!(
            conflict_vars,
            assigned
                .get()
                .intersection(assigned.get().map(Not::not))
                .map(var)
                .consolidate()
        );

        // Interrupt early when a direct conflict is detected
        db.interrupt(conflict_vars.get());

        // === Compute Units ===
        // Clauses with at least one true literal are satisfied
        assign!(
            satisfied_clause_ids,
            all_clauses
                .get()
                .swap()
                .semijoin(assigned.get())
                .snd()
                .consolidate()
        );
        assign_saved!(
            unsatisfied_clause_ids,
            all_clause_ids.difference(satisfied_clause_ids)
        );
        // Remaining literals: unassigned literals in unsatisfied clauses.
        // These are exactly the literals that could still satisfy their clause.
        // - semijoin with unsatisfied_clauses: only consider clauses not yet satisfied
        // - antijoin with negated assigned: exclude falsified literals
        // Since satisfied clauses are excluded, no literal here can be assigned true.
        assign_saved!(
            remaining_clause_literals,
            all_clauses
                .get()
                .semijoin(unsatisfied_clause_ids.get())
                .swap()
                .antijoin(assigned.get().map(Not::not))
                .swap()
        );
        // Empty clauses: unsatisfied clauses with no remaining literals (all falsified)
        assign_saved!(
            empty_clauses,
            unsatisfied_clause_ids
                .get()
                .difference(remaining_clause_literals.get().fst().consolidate())
                .consolidate()
        );

        // Interrupt early when an empty clause is detected
        db.interrupt(empty_clauses.get());

        // Count remaining literals per clause to find unit clauses
        assign!(
            remaining_clause_sizes,
            remaining_clause_literals.get().fst().consolidate().counts()
        );

        // Count how many clauses each literal appears in (for decision heuristics)
        assign!(
            literal_counts,
            remaining_clause_literals.get().snd().consolidate().counts()
        );

        // Unit clauses: exactly one remaining literal (must be assigned true)
        assign!(
            units,
            remaining_clause_literals
                .get()
                .semijoin(
                    remaining_clause_sizes
                        .filter(|&(_cid, size)| size == 1)
                        .map(|(cid, _)| cid)
                )
                .consolidate()
        );

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

        // Combine both conflict types: direct conflicts (x and !x assigned)
        // and empty clauses (all literals in a clause falsified)
        assign!(
            conflicts,
            conflict_vars
                .get()
                .map(Conflict::DirectConflict)
                .union(empty_clauses.get().map(Conflict::EmptyClause))
        );

        // Create outputs from relations (need to box them to store in struct)
        let assignments_out = assignments.get().output_with_sink();
        let causes_out = causes.output_with_sink();
        let assigned_out = assigned.get().output();
        let conflicts_out = conflicts.output();
        let literal_counts_out = literal_counts.output_with_sink();

        // Initialize with Level::TOP so unit propagation works at level 0
        levels.insert(Level::TOP);

        Solver {
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
                literal_counts: literal_counts_out,
            },
            state: State {
                current_level: Level::TOP,
                next_learned_id: ClauseId::new(1),
                decision_stack: Vec::new(),
                clause_db: HashMap::new(),
            },
        }
    }
}
