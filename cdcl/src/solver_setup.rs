//! CDCL Solver dataflow setup and constructor.

use std::hash::Hash;
use std::ops::Not;

use ahash::RandomState;
use contiguous_data::HashSet;
use either::Either;
use relational::database::{CommitId, DatabaseBuilder, Op, Relation};
use relational::{
    assign, assign_and_interrupt, assign_partition, assign_saved, create_input,
    create_persistent_input, create_variable,
};

/// Seeded hash for deterministic ordering.
fn seeded_hash<T: Hash>(val: &T, seed: u64) -> u64 {
    let build_hasher = RandomState::with_seeds(seed, 0, 0, 0);
    build_hasher.hash_one(val)
}

const CAUSE_SEED: u64 = 0x1f2e3d4c5b6a7980;

use crate::clause_deletion::ClauseDeletion;
use crate::restart::RestartStrategy;
use crate::types::Conflict;
use crate::vsids::Vsids;

use super::solver::{Inputs, Outputs, Solver, State};
use super::types::{Cause, ConstraintId, Level, Lit, Var, Weight};

impl Solver {
    /// Create a new solver using the provided database builder.
    ///
    /// `vars` is the set of decision variables in the problem.
    /// `external` is a relation of pre-assigned literals for external variables.
    /// External variables participate in unit propagation and conflict analysis
    /// but cannot be selected as decision literals.
    ///
    /// The caller is responsible for calling `db.build()` after this returns
    /// and passing the resulting `&mut Database` to solver methods.
    pub fn new(
        db: &mut DatabaseBuilder,
        vars: &HashSet<Var>,
        external: Relation<impl Op<Lit> + 'static>,
    ) -> Self {
        // === Input Relations ===
        // PB constraint terms: (constraint_id, literal, weight)
        create_persistent_input!(db, terms_inp, terms, (ConstraintId, Lit, Weight));
        // PB constraint bounds: (constraint_id, bound)
        create_persistent_input!(db, bounds_inp, bounds, (ConstraintId, Weight));
        // Learned constraints
        create_persistent_input!(db, learned_terms_inp, learned_terms, (ConstraintId, Lit, Weight));
        create_persistent_input!(db, learned_bounds_inp, learned_bounds, (ConstraintId, Weight));
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

        // Save learned for output and concat
        let learned_terms = learned_terms.save();
        let learned_bounds = learned_bounds.save();

        // All constraint terms and bounds (original + learned)
        assign_saved!(all_terms = terms.concat(learned_terms.get()));
        assign_saved!(all_bounds = bounds.concat(learned_bounds.get()));

        // === Feedback-based Unit Propagation ===
        // prep accumulates ((Lit, Level, Cause), CommitId) via feedback_with_id
        create_variable!(db, prep_var, prep, ((Lit, Level, Cause), CommitId));
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
                .map(|((lit, level, cause), commit_id)| (
                    lit,
                    (
                        level,
                        commit_id,
                        seeded_hash(&(lit, level, cause), CAUSE_SEED),
                        cause
                    )
                ))
                .group_min()
        );

        assign_partition!(
            (analysis_start_constraint_id, analysis_start_var) =
                analysis.map(|conflict| match conflict {
                    Conflict::UnsatConstraint(cid) => Either::Left(cid),
                    Conflict::DirectConflict(var) => Either::Right(var),
                })
        );
        let analysis_start_constraint_id = analysis_start_constraint_id.save();
        assign!(
            analysis_start_var_lits = analysis_start_var.flat_map(|v| [Lit::pos(v), Lit::neg(v)])
        );
        // Get literals from conflicting constraint (negated, since they were falsified)
        assign!(
            analysis_start_constraint_lits = all_terms
                .get()
                .map(|(cid, lit, _weight)| (cid, lit))
                .semijoin(analysis_start_constraint_id.get())
                .snd()
                .map(Not::not)
        );
        assign_saved!(
            analysis_start_lits = analysis_start_var_lits.concat(analysis_start_constraint_lits)
        );
        assign!(
            analysis_level = causes
                .get()
                .semijoin(analysis_start_lits.get())
                .map(|(_lit, (level, _, _, _))| level)
                .consolidate()
                .global_max()
        );
        create_variable!(db, analysis_lits_var, analysis_lits, Lit);
        assign!(analysis_lit_causes = causes.get().semijoin(analysis_lits));
        assign_partition!(
            (on_level, below_level) = analysis_lit_causes.cartesian_product(analysis_level).map(
                |(entry @ (lit, (level, _, _, _)), max_level)| {
                    if level == max_level {
                        Either::Left(entry)
                    } else {
                        Either::Right((!lit, level))
                    }
                }
            )
        );
        let on_level = on_level.save();
        assign!(min_lit_on_level = on_level.get().swap().global_min().snd().consolidate());
        assign_partition!(
            (analysis_constraint_ids, level_retained) = on_level
                .get()
                .cartesian_product(min_lit_on_level)
                .filter_map(|((lit, (level, commit_id, _, cause)), min_lit)| {
                    let retain = match level {
                        Level::TOP => cause == Cause::NoConstraint,
                        _ => lit == min_lit,
                    };
                    if retain {
                        Some(Either::Right((!lit, level)))
                    } else {
                        match cause {
                            Cause::NoConstraint => None,
                            // Include the commit_id as the cutoff for filtering
                            Cause::FromConstraint(cid) => Some(Either::Left((cid, commit_id))),
                        }
                    }
                })
        );
        let analysis_constraint_ids = analysis_constraint_ids.save();
        // Get literals from constraints, but only those assigned BEFORE the explained literal
        assign!(
            analysis_new_lits = all_terms
                .get()
                .map(|(cid, lit, _weight)| (cid, !lit))
                .join(analysis_constraint_ids.get()) // (cid, (neg_lit, explain_commit_id))
                .snd() // (neg_lit, explain_commit_id)
                .join(causes.get().map(|(lit, (_, commit_id, _, _))| (lit, commit_id)))
                // Now: (neg_lit, (explain_commit_id, lit_commit_id))
                .filter_map(|(neg_lit, (explain_commit_id, lit_commit_id))| {
                    // Only include if this literal was assigned BEFORE the explained literal
                    (lit_commit_id < explain_commit_id).then_some(neg_lit)
                })
                .consolidate()
        );
        db.feedback(
            analysis_lits_var,
            analysis_start_lits.get().concat(analysis_new_lits),
        );
        assign!(new_clause = below_level.concat(level_retained));

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

        // === PB Constraint Propagation ===
        // For each constraint, compute:
        // - true_weight: sum of weights of satisfied literals
        // - remaining: (constraint_id, lit, weight) for unassigned literals
        // - slack = true_weight + remaining_weight - bound
        // Conflict if slack < 0
        // Propagate literal if slack < weight of that literal

        // Get all constraint IDs from bounds
        assign_saved!(all_constraint_ids = all_bounds.get().fst().consolidate());

        // Weight contributed by satisfied literals: (cid, weight)
        assign!(
            true_weight_per_term = all_terms
                .get()
                .map(|(cid, lit, weight)| (lit, (cid, weight)))
                .semijoin(assigned.get())
                .snd()
        );
        // Sum true weights per constraint (only for constraints with satisfied literals)
        assign_saved!(true_weight_nonzero = true_weight_per_term.group_sum());
        // Add zero entries for constraints without satisfied literals
        assign!(cids_with_true_weight = true_weight_nonzero.get().fst().consolidate());
        assign!(
            true_weight_zero = all_constraint_ids
                .get()
                .set_minus(cids_with_true_weight)
                .map(|cid| (cid, 0i64))
        );
        assign!(true_weight_per_constraint = true_weight_nonzero.get().concat(true_weight_zero));

        // Remaining terms (unassigned and not falsified): (cid, lit, weight)
        assign_saved!(
            remaining_terms = all_terms
                .get()
                .map(|(cid, lit, weight)| (lit, (cid, weight)))
                .antijoin(assigned.get())                          // not satisfied
                .antijoin(assigned.get().map(Not::not))            // not falsified
                .map(|(lit, (cid, weight))| (cid, lit, weight))
        );

        // Sum remaining weights per constraint (only for constraints with remaining literals)
        assign_saved!(remaining_weight_nonzero = remaining_terms
            .get()
            .map(|(cid, _lit, weight)| (cid, weight))
            .group_sum()
        );
        // Add zero entries for constraints without remaining literals
        assign!(cids_with_remaining_weight = remaining_weight_nonzero.get().fst().consolidate());
        assign!(
            remaining_weight_zero = all_constraint_ids
                .get()
                .set_minus(cids_with_remaining_weight)
                .map(|cid| (cid, 0i64))
        );
        assign!(
            remaining_weight_per_constraint =
                remaining_weight_nonzero.get().concat(remaining_weight_zero)
        );

        // Compute slack per constraint: true_weight + remaining_weight - bound
        // slack = (cid, slack_value) where slack_value can be negative
        assign_saved!(
            slack_per_constraint = all_bounds
                .get()
                .join(true_weight_per_constraint)
                .join(remaining_weight_per_constraint)
                .map(|(cid, ((bound, true_w), remaining_w))| {
                    let slack = true_w + remaining_w - bound;
                    (cid, slack)
                })
        );

        // Conflict: constraints where slack < 0
        assign_and_interrupt!(
            db,
            unsat_constraints = slack_per_constraint
                .get()
                .filter_map(|(cid, slack)| (slack < 0).then_some(cid))
        );

        // Propagation: literals that must be true because slack < weight
        // (cid, lit) where lit must be assigned true
        assign!(
            propagate_lits = remaining_terms
                .get()
                .map(|(cid, lit, weight)| (cid, (lit, weight)))
                .join(slack_per_constraint.get())
                .filter_map(|(cid, ((lit, weight), slack))| {
                    (slack < weight).then_some((cid, lit))
                })
        );

        // === Set up the feedback loop ===
        assign!(propagate_with_level = propagate_lits.cartesian_product(current_level.get()));
        assign!(
            propagate_lit_level_cause = propagate_with_level
                .map(|((cid, lit), level)| (lit, level, Cause::FromConstraint(cid)))
        );

        // External literals are assigned at Level::TOP with no constraint cause
        assign!(external_assignments = external.map(|lit| (lit, Level::TOP, Cause::NoConstraint)));

        assign!(
            all_new_assignments = decision_assignments
                .map(|(lit, level)| (lit, level, Cause::NoConstraint))
                .concat(propagate_lit_level_cause)
                .concat(external_assignments)
        );

        db.feedback_with_id(prep_var, all_new_assignments);

        // Combine both conflict types: direct conflicts (x and !x assigned)
        // and unsatisfied constraints
        assign!(
            conflicts = conflict_vars
                .map(Conflict::DirectConflict)
                .concat(unsat_constraints.map(Conflict::UnsatConstraint))
        );

        assign!(
            this_level_assignments = assignments.get().swap().semijoin(current_level.get()).snd()
        );

        // Create outputs from relations
        // Initialize with Level::TOP so unit propagation works at level 0
        levels_inp.insert(Level::TOP);

        Solver {
            inputs: Inputs {
                analysis: analysis_inp,
                terms: terms_inp,
                bounds: bounds_inp,
                learned_terms: learned_terms_inp,
                learned_bounds: learned_bounds_inp,
                levels: levels_inp,
                decision_assignments: decision_assignments_inp,
            },
            outputs: Outputs {
                assigned: assigned.get().output(),
                assigned_saved: assigned,
                assignment_levels: assignments.get().output_with_sink(),
                new_clause: new_clause.boxed().output_with_sink(),
                analysis_constraint_ids: analysis_constraint_ids
                    .get()
                    .fst() // Extract just the ConstraintId from (ConstraintId, CommitId)
                    .concat(analysis_start_constraint_id.get())
                    .boxed()
                    .output(),
                conflicts: conflicts.boxed().output_with_sink(),
                this_level_assignments: this_level_assignments.boxed().output(),
                learned_terms: learned_terms.get().boxed().output(),
                learned_bounds: learned_bounds.get().boxed().output(),
            },
            state: State {
                current_level: Level::TOP,
                next_learned_id: 0,
                restart: RestartStrategy::new(100), // Restart after 100*luby(i) conflicts
                clause_deletion: ClauseDeletion::new(),
                vsids: Vsids::new(vars),
                has_empty_clause: false,
            },
        }
    }
}
