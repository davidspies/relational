//! CDCL Solver dataflow setup and constructor.

use std::hash::Hash;
use std::ops::Not;

use ahash::{AHashMap, RandomState};
use either::Either;
use relational::database::{CommitId, DatabaseBuilder};
use relational::{
    assign, assign_and_interrupt, assign_saved, create_input, create_persistent_input,
    create_variable,
};

/// Seeded hash for deterministic ordering.
fn seeded_hash<T: Hash>(val: &T, seed: u64) -> u64 {
    let build_hasher = RandomState::with_seeds(seed, 0, 0, 0);
    build_hasher.hash_one(val)
}

const WATCH_SEED: u64 = 0x7a3b9c1d4e5f6028;
const CAUSE_SEED: u64 = 0x1f2e3d4c5b6a7980; // For deterministic cause selection

use crate::clause_deletion::ClauseDeletion;
use crate::restart::RestartStrategy;
use crate::types::Conflict;
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
        create_input!(db, clauses_inp, clauses, (ClauseId, Lit));
        create_persistent_input!(db, learned_inp, learned, (ClauseId, Lit));
        create_input!(db, mut levels_inp, levels, Level);
        create_input!(
            db,
            decision_assignments_inp,
            decision_assignments,
            (Lit, Level)
        );
        create_input!(db, analysis_inp, analysis, Conflict);

        // Current level = max(levels)
        assign_saved!(current_level = levels.global_max());

        // All clauses (original + learned)
        assign_saved!(all_clauses = clauses.concat(learned));

        // === Feedback-based Unit Propagation ===
        // prep accumulates ((Lit, Level, ClauseId), CommitId) via feedback_with_id
        create_variable!(db, prep_var, prep, ((Lit, Level, ClauseId), CommitId));
        let prep = prep.save();

        // Final assignments: for each literal, take the entry with minimum CommitId
        assign_saved!(
            assignments = prep
                .get()
                .map(|((lit, level, _cid), _id)| (lit, level))
                .consolidate()
                .group_min()
        );

        // Derived: which literals are assigned true
        assign_saved!(assigned = assignments.get().fst());

        assign_saved!(
            causes = prep
                .get()
                .map(|((lit, level, clause_id), commit_id)| (
                    lit,
                    (
                        level,
                        commit_id,
                        seeded_hash(&(lit, level, clause_id), CAUSE_SEED),
                        clause_id
                    )
                ))
                .group_min()
                .map(|(lit, (level, _commit_id, _hash, clause_id))| (lit, (level, clause_id)))
        );

        let (analysis_start_clause_id, analysis_start_var) = analysis
            .map(|conflict| match conflict {
                Conflict::EmptyClause(clause_id) => Either::Left(clause_id),
                Conflict::DirectConflict(var) => Either::Right(var),
            })
            .partition();
        assign!(
            analysis_start_var_lits = analysis_start_var.flat_map(|v| [Lit::pos(v), Lit::neg(v)])
        );
        assign!(
            analysis_start_clause_lits = all_clauses
                .get()
                .semijoin(analysis_start_clause_id)
                .snd()
                .map(Not::not)
        );
        assign!(analysis_start_lits = analysis_start_var_lits.concat(analysis_start_clause_lits));
        create_variable!(db, analysis_lits_var, analysis_lits, Lit);
        assign_saved!(analysis_lit_causes = causes.get().semijoin(analysis_lits));
        assign!(
            analysis_level = analysis_lit_causes
                .get()
                .map(|(_lit, (level, _clause_id))| level)
                .consolidate()
                .global_max()
        );
        let (on_level, new_clause) = analysis_lit_causes
            .get()
            .cartesian_product(analysis_level)
            .map(|((lit, (level, clause_id)), max_level)| {
                if level == max_level && clause_id != ClauseId::DECISION {
                    Either::Left(clause_id)
                } else {
                    Either::Right((!lit, level))
                }
            })
            .partition();
        assign!(
            analysis_new_lits = all_clauses
                .get()
                .semijoin(on_level)
                .snd()
                .consolidate()
                .map(Not::not)
                .intersection(assigned.get())
        );
        db.feedback(
            analysis_lits_var,
            analysis_start_lits.concat(analysis_new_lits),
        );

        // === Conflict Detection ===
        // Direct conflict: both a literal and its negation are assigned
        // Interrupt early when a direct conflict is detected
        assign_and_interrupt!(
            db,
            conflict_vars = assigned
                .get()
                .intersection(assigned.get().map(Not::not))
                .map(|lit| lit.var())
        );

        // === Watched Literals ===
        // For each clause, select 2 literals deterministically using hash-based ordering.
        // This gives us (clause_id, ArrayVec<(hash, lit), 2>).

        assign!(
            hashed_clause_literals = all_clauses.get().map(|entry| {
                let hash = seeded_hash(&entry, WATCH_SEED);
                (entry, hash)
            })
        );

        create_variable!(db, satisfied_clause_ids_var, satisfied_clause_ids, ClauseId);
        let satisfied_clause_ids = satisfied_clause_ids.save();

        create_variable!(
            db,
            removed_assignments_var,
            removed_assignments,
            (ClauseId, Lit)
        );

        assign_saved!(
            grouped_watched_literals_satisfiable = hashed_clause_literals
                .antijoin(removed_assignments)
                .map(|((cid, lit), hash)| (cid, (hash, lit)))
                .group_min_n::<_, _, 2>()
        );

        assign_saved!(
            grouped_watched_literals = grouped_watched_literals_satisfiable
                .get()
                .antijoin(satisfied_clause_ids.get())
        );

        assign_saved!(
            watched_literals = grouped_watched_literals
                .get()
                .flat_map(|(cid, arr)| arr.into_iter().map(move |(_, lit)| (cid, lit)))
        );

        // A clause is satisfiable if it is satisfied or has at least one watched literal unassigned
        assign!(
            satisfiable_clause_ids = grouped_watched_literals_satisfiable
                .get()
                .fst()
                .consolidate()
        );

        assign!(all_clause_ids = all_clauses.get().fst().consolidate());
        // Empty clauses: Clauses which are no longer satisfiable
        // Interrupt early when an empty clause is detected
        assign_and_interrupt!(
            db,
            empty_clauses = all_clause_ids.set_minus(satisfiable_clause_ids)
        );

        // === Compute Units ===
        // Clauses with at least one true literal are satisfied
        db.feedback(
            satisfied_clause_ids_var,
            watched_literals.get().swap().semijoin(assigned.get()).snd(),
        );
        // False literals must be removed to make room for new watched literals
        db.feedback(
            removed_assignments_var,
            watched_literals
                .get()
                .swap()
                .semijoin(assigned.get().map(Not::not))
                .swap(),
        );

        // Unit clauses: exactly one remaining literal (must be assigned true)
        assign_saved!(
            units = grouped_watched_literals.get().filter_map(|(cid, arr)| {
                let mut iter = arr.into_iter();
                let (_hash, lit) = iter.next().unwrap();
                iter.next().is_none().then_some((cid, lit))
            })
        );

        // === Set up the feedback loop ===
        assign!(unit_with_level = units.get().cartesian_product(current_level.get()));
        assign!(unit_lit_level_cid = unit_with_level.map(|((cid, lit), level)| (lit, level, cid)));

        assign!(
            all_new_assignments = decision_assignments
                .map(|(lit, level)| (lit, level, ClauseId::DECISION))
                .concat(unit_lit_level_cid)
        );

        db.feedback_with_id(prep_var, all_new_assignments);

        // Combine both conflict types: direct conflicts (x and !x assigned)
        // and empty clauses (all literals in a clause falsified)
        assign!(
            conflicts = conflict_vars
                .map(Conflict::DirectConflict)
                .concat(empty_clauses.map(Conflict::EmptyClause))
        );

        assign!(
            this_level_assignments = assignments.get().swap().semijoin(current_level.get()).snd()
        );

        // Create outputs from relations
        let new_clause_out = new_clause.boxed().output_with_sink();
        let conflicts_out = conflicts.boxed().output_with_sink();
        let this_level_assignments_out = this_level_assignments.boxed().output();
        let assigned_out = assigned.get().output();

        // Initialize with Level::TOP so unit propagation works at level 0
        levels_inp.insert(Level::TOP);

        Solver {
            inputs: Inputs {
                analysis: analysis_inp,
                clauses: clauses_inp,
                learned: learned_inp,
                levels: levels_inp,
                decision_assignments: decision_assignments_inp,
            },
            outputs: Outputs {
                assigned: assigned_out,
                new_clause: new_clause_out,
                conflicts: conflicts_out,
                this_level_assignments: this_level_assignments_out,
            },
            state: State {
                current_level: Level::TOP,
                next_learned_id: ClauseId::new(1),
                clause_db: AHashMap::new(),
                restart: RestartStrategy::new(100), // Restart after 100*luby(i) conflicts
                clause_deletion: ClauseDeletion::new(),
                vsids: Vsids::new(num_vars),
            },
        }
    }
}
