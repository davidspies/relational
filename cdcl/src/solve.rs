//! Main solve loop for the CDCL solver.

use std::time::Instant;

use relational::database::Database;

use super::Solver;
use super::proof::ProofWriter;
use super::types::Level;

/// Statistics for debugging/profiling.
#[derive(Default)]
pub struct SolveStats {
    pub decisions: u64,
    pub conflicts: u64,
    pub restarts: u64,
}

impl Solver {
    /// Main solve loop with CDCL (Conflict-Driven Clause Learning).
    ///
    /// Uses 1-UIP conflict analysis to learn clauses and perform
    /// non-chronological backtracking.
    pub fn solve(&mut self, db: &mut Database) -> bool {
        self.solve_with_proof(db, None)
    }

    /// Solve with optional DRAT proof logging.
    pub fn solve_with_proof(&mut self, db: &mut Database, proof: Option<&mut ProofWriter>) -> bool {
        let start = Instant::now();
        let (result, stats) = self.solve_internal(db, proof);
        eprintln!(
            "c stats: {:.3}s decisions={} conflicts={} restarts={}",
            start.elapsed().as_secs_f64(),
            stats.decisions,
            stats.conflicts,
            stats.restarts
        );
        result
    }

    fn solve_internal(
        &mut self,
        db: &mut Database,
        mut proof: Option<&mut ProofWriter>,
    ) -> (bool, SolveStats) {
        let mut stats = SolveStats::default();
        let start = Instant::now();
        let mut last_report = start;
        loop {
            // Periodic progress report every 5 seconds
            let now = Instant::now();
            if now.duration_since(last_report).as_secs() >= 5 {
                let causes = self.outputs.causes.get();
                let level = self.state.current_level.raw();
                let fixed = causes.count_at_level(Level::TOP);
                let total_non_fixed = self.state.num_vars as usize - fixed;
                let assigned_non_fixed = causes.count_non_fixed();
                let learned = self.state.clause_deletion.len();
                let elapsed = start.elapsed().as_secs_f64();
                let SolveStats {
                    decisions,
                    conflicts,
                    restarts,
                } = stats;
                eprintln!(
                    "c progress: {elapsed:.1}s decisions={decisions} conflicts={conflicts} restarts={restarts} \
                    learned={learned} level={level} assigned={assigned_non_fixed}/{total_non_fixed}",
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
                    match self.analyze_conflict(conflict) {
                        None => {
                            // Conflict at level 0 = UNSAT
                            if let Some(ref mut p) = proof {
                                let _ = p.add_empty_clause();
                                let _ = p.flush();
                            }
                            return (false, stats);
                        }
                        Some(analysis) => {
                            // Log the learned clause to proof
                            if let Some(ref mut p) = proof {
                                let _ = p.add_clause(&analysis.learned_clause);
                            }

                            // Bump VSIDS activity for variables in learned clause
                            for lit in &analysis.learned_clause {
                                self.state.vsids.bump(lit.var());
                            }
                            self.state.vsids.decay();

                            // Non-chronological backtrack to the computed level FIRST
                            self.backtrack_to(db, analysis.backtrack_level);

                            // Then learn the clause with LBD tracking
                            self.learn_clause_with_levels(
                                db,
                                &analysis.learned_clause,
                                &analysis.learned_clause_levels,
                            );

                            // Decay clause activities
                            self.state.clause_deletion.decay_activities();

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
    }
}
