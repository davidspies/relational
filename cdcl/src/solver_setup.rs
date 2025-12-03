//! CDCL Solver dataflow setup and constructor.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Not;

use relational::database::{CommitId, DatabaseBuilder};
use relational::{assign, assign_saved, create_input, create_persistent_input, create_variable};

/// Seeded hash for deterministic watched literal selection.
fn seeded_hash<T: Hash>(val: &T, seed: u64) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    seed.hash(&mut hasher);
    val.hash(&mut hasher);
    hasher.finish()
}

const WATCH_SEED: u64 = 0x7a3b9c1d4e5f6028;

use crate::Conflict;
use crate::clause_deletion::ClauseDeletion;
use crate::restart::RestartStrategy;
use crate::types::var;
use crate::vsids::Vsids;

use super::solver::{Inputs, Outputs, Solver, State};
use super::types::{ClauseId, Level, Lit};

impl Solver {
    /// Create a new solver using the provided database builder.
    ///
    /// `num_vars` is the number of variables in the problem.
    /// The caller is responsible for calling `db.build()` after this returns
    /// and passing the resulting `&mut Database` to solver methods.
    pub fn new(db: &mut DatabaseBuilder, num_vars: u32) -> Self {
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
        assign_saved!(current_level_rel, levels_rel.global_max());

        // All clauses (original + learned)
        assign_saved!(all_clauses, clauses_rel.union(learned_rel));

        assign_saved!(
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

        // === Watched Literals ===
        // For each clause, select 2 literals deterministically using hash-based ordering.
        // This gives us (clause_id, ArrayVec<(hash, lit), 2>).

        create_variable!(db, satisfied_clause_ids, satisfied_clause_ids_rel, ClauseId);
        assign_saved!(satisfied_clause_ids_rel, satisfied_clause_ids_rel);

        create_variable!(
            db,
            removed_assignments,
            removed_assignments_rel,
            (ClauseId, Lit)
        );

        assign_saved!(
            grouped_watched_literals,
            all_clauses
                .get()
                .map(|(cid, lit)| {
                    let hash = seeded_hash(&(cid, lit), WATCH_SEED);
                    ((cid, lit), hash)
                })
                .antijoin(removed_assignments_rel)
                .map(|((cid, lit), hash)| (cid, (hash, lit)))
                .group_min_n::<_, _, 2>()
                .consolidate()
                .antijoin(satisfied_clause_ids_rel.get())
        );

        assign_saved!(
            watched_literals,
            grouped_watched_literals
                .get()
                .flat_map(|(cid, arr)| { arr.into_iter().map(move |(_, lit)| (cid, lit)) })
                .consolidate()
        );

        // === Compute Units ===
        // Clauses with at least one true literal are satisfied
        db.feedback(
            satisfied_clause_ids,
            watched_literals
                .get()
                .swap()
                .semijoin(assigned.get())
                .snd()
                .consolidate(),
        );
        // False literals must be removed to make room for new watched literals
        db.feedback(
            removed_assignments,
            watched_literals
                .get()
                .swap()
                .semijoin(assigned.get().map(Not::not))
                .consolidate()
                .swap(),
        );

        // Empty clauses: unsatisfied clauses with no remaining literals (all falsified)
        assign!(
            possibly_unsatisfied_clause_ids,
            all_clause_ids
                .get()
                .difference(satisfied_clause_ids_rel.get())
        );

        assign_saved!(
            empty_clauses,
            possibly_unsatisfied_clause_ids
                .difference(grouped_watched_literals.get().fst().consolidate())
                .consolidate()
        );

        // Interrupt early when an empty clause is detected
        db.interrupt(empty_clauses.get());

        // Unit clauses: exactly one remaining literal (must be assigned true)
        assign!(
            units,
            grouped_watched_literals
                .get()
                .filter(|(_, arr)| arr.len() == 1)
                .map(|(cid, arr)| {
                    let (_hash, lit) = arr[0];
                    (cid, lit)
                })
        );

        // === Set up the feedback loop ===
        assign!(
            unit_with_level,
            units.cartesian_product(current_level_rel.get())
        );
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

        assign!(
            this_level_assignments,
            assignments
                .get()
                .swap()
                .semijoin(current_level_rel.get())
                .snd()
                .consolidate()
        );

        // Create outputs from relations (need to box them to store in struct)
        let causes_out = causes.output_with_sink();
        let conflicts_out = conflicts.output_with_sink();
        let this_level_assignments_out = this_level_assignments.output();

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
                causes: causes_out,
                conflicts: conflicts_out,
                this_level_assignments: this_level_assignments_out,
            },
            state: State {
                current_level: Level::TOP,
                next_learned_id: ClauseId::new(1),
                decision_stack: Vec::new(),
                clause_db: HashMap::new(),
                restart: RestartStrategy::new(100), // Restart after 100*luby(i) conflicts
                clause_deletion: ClauseDeletion::new(),
                vsids: Vsids::new(num_vars),
            },
        }
    }
}
