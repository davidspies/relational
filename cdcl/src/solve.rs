//! Main solve loop for the CDCL solver.

use std::cell::Ref;
use std::time::Instant;

use contiguous_data::{HashMap, HashSet, Multiset};
use relational::database::Database;

use crate::Lit;
use crate::conflict_analysis::AnalysisResult;
use crate::types::{Level, Var};

use super::Solver;
use super::proof::ProofWriter;

/// Statistics for debugging/profiling.
#[derive(Default)]
pub struct SolveStats {
    pub decisions: u64,
    pub conflicts: u64,
    pub restarts: u64,
}

/// The result of solving: either a satisfying assignment or UNSAT.
pub type SolveResult = Option<HashMap<Var, bool>>;

impl Solver {
    /// Main solve loop with CDCL (Conflict-Driven Clause Learning).
    ///
    /// Returns `Some(assignment)` if SAT, `None` if UNSAT.
    /// Uses 1-UIP conflict analysis to learn clauses and perform
    /// non-chronological backtracking.
    pub fn solve(&mut self, db: &mut Database) -> SolveResult {
        self.solve_with_proof(db, None)
    }

    /// Solve but keep the decision stack on SAT (don't backtrack).
    /// Returns (is_sat, stats).
    /// Use `get_assignment()` to get the current assignment after SAT.
    pub fn solve_and_stay(&mut self, db: &mut Database) -> (bool, SolveStats) {
        self.solve_core(db, None)
    }

    /// Get the current assignment (all assigned literals).
    pub fn get_assignment(&self) -> Ref<'_, Multiset<Lit>> {
        self.outputs.assigned.get()
    }

    /// Get the current decision level.
    pub fn current_level(&self) -> Level {
        self.state.current_level
    }

    /// Backtrack to a specific level (public for incremental solving).
    pub fn backtrack(&mut self, db: &mut Database, level: Level) {
        self.backtrack_to(db, level);
    }

    /// Sanity check: every decision variable should be either assigned or selectable.
    ///
    /// Returns Ok(()) if the invariant holds, Err with details if it doesn't.
    pub fn sanity_check_vars(&self, decision_vars: &HashSet<Var>) -> Result<(), String> {
        let assigned: HashSet<Var> = self
            .outputs
            .assigned
            .get()
            .iter()
            .map(|lit| lit.var())
            .collect();

        // Check which vars are in VSIDS queue vs stashed
        let in_queue: HashSet<Var> = self.state.vsids.queue_vars();
        let in_stashed: HashSet<Var> = self.state.vsids.stashed_vars();

        let mut errors = Vec::new();

        for &var in decision_vars {
            let is_assigned = assigned.contains(&var);
            let is_in_queue = in_queue.contains(&var);
            let is_in_stashed = in_stashed.contains(&var);

            // Variable must be reachable: either assigned, in queue, or in stashed
            // Note: assigned + in_queue is OK (queue uses lazy cleanup)
            // Note: assigned + stashed is OK (stashed means "was assigned via pick")
            let reachable = is_assigned || is_in_queue || is_in_stashed;

            if !reachable {
                errors.push(format!(
                    "var {} is MISSING: not assigned, not in queue, not in stashed",
                    var.raw()
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Solve with optional DRAT proof logging.
    ///
    /// Returns `Some(assignment)` if SAT, `None` if UNSAT.
    pub fn solve_with_proof(
        &mut self,
        db: &mut Database,
        proof: Option<&mut ProofWriter>,
    ) -> SolveResult {
        let start = Instant::now();
        let (sat, stats) = self.solve_core(db, proof);
        eprintln!(
            "c stats: {:.3}s decisions={} conflicts={} restarts={}",
            start.elapsed().as_secs_f64(),
            stats.decisions,
            stats.conflicts,
            stats.restarts
        );
        if sat {
            let assignment = self
                .outputs
                .assigned
                .get()
                .iter()
                .map(|&lit| (lit.var(), lit.is_positive()))
                .collect();
            self.backtrack_to(db, Level::TOP);
            Some(assignment)
        } else {
            None
        }
    }

    /// Core solve loop. Returns (is_sat, stats). Does NOT backtrack on SAT.
    fn solve_core(
        &mut self,
        db: &mut Database,
        mut proof: Option<&mut ProofWriter>,
    ) -> (bool, SolveStats) {
        let mut stats = SolveStats::default();

        // Check for empty clause (immediate UNSAT)
        if self.state.has_empty_clause {
            return (false, stats);
        }

        let start = Instant::now();
        let mut last_report = start;
        loop {
            // Periodic progress report every 5 seconds
            let now = Instant::now();
            if now.duration_since(last_report).as_secs() >= 5 {
                let level = self.state.current_level.raw();
                let learned = self.state.clause_deletion.len();
                let elapsed = start.elapsed().as_secs_f64();
                let SolveStats {
                    decisions,
                    conflicts,
                    restarts,
                } = stats;
                eprintln!(
                    "c progress: {elapsed:.1}s decisions={decisions} conflicts={conflicts} restarts={restarts} \
                    learned={learned} level={level}",
                );
                last_report = now;
            }
            // Propagate
            match self.propagate() {
                Ok(()) => {
                    // No conflict - pick next literal or return SAT
                    match self.pick_branching_literal() {
                        Some(lit) => {
                            stats.decisions += 1;
                            self.decide(db, lit);
                        }
                        None => {
                            // All variables assigned, no conflict = SAT
                            return (true, stats);
                        }
                    }
                }
                Err(conflict) => {
                    stats.conflicts += 1;
                    // Conflict! Analyze and learn.
                    let AnalysisResult {
                        learned_clause,
                        conflict_level,
                        backtrack_level,
                    } = self.analyze_conflict(db, conflict);

                    // Log the learned clause to proof
                    if let Some(ref mut p) = proof {
                        let lits: Vec<_> = learned_clause.iter().map(|&(lit, _)| lit).collect();
                        let _ = p.add_clause(&lits);
                    }

                    // Bump VSIDS activity for variables in learned clause
                    for &(lit, _) in &learned_clause {
                        self.state.vsids.bump(lit.var());
                    }
                    self.state.vsids.decay();

                    // Non-chronological backtrack to the computed level FIRST
                    self.backtrack_to(db, backtrack_level);

                    // Then learn the clause with LBD tracking
                    self.learn_clause(db, &learned_clause);

                    // Decay clause activities
                    self.state.clause_deletion.decay_activities();

                    if conflict_level == Level::TOP {
                        // Conflict at level 0 = UNSAT
                        if let Some(ref mut p) = proof {
                            let _ = p.flush();
                        }
                        return (false, stats);
                    }

                    // Check if we should restart
                    if self.state.restart.on_conflict() {
                        stats.restarts += 1;
                        self.restart(db);
                        // Good time to clean up learned clauses
                        self.maybe_delete_clauses(db);
                    }

                    // The learned clause is now unit (asserting), so propagation
                    // will assign the UIP literal on the next iteration
                }
            }
        }
    }
}
