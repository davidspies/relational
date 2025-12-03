//! Main solve loop for the CDCL solver.

use relational::database::Database;

use super::Solver;
use super::proof::ProofWriter;

/// Statistics for debugging/profiling.
#[derive(Default)]
pub struct SolveStats {
    pub decisions: u64,
    pub conflicts: u64,
    pub restarts: u64,
    pub heap_rebuilds: u64,
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
        let (result, stats) = self.solve_internal(db, proof);
        eprintln!(
            "c stats: decisions={} conflicts={} restarts={} heap_rebuilds={}",
            stats.decisions, stats.conflicts, stats.restarts, stats.heap_rebuilds
        );
        result
    }

    fn solve_internal(
        &mut self,
        db: &mut Database,
        mut proof: Option<&mut ProofWriter>,
    ) -> (bool, SolveStats) {
        let mut stats = SolveStats::default();
        let mut last_report = 0u64;
        loop {
            // Periodic progress report
            if stats.conflicts >= last_report + 10000 {
                eprintln!(
                    "c progress: decisions={} conflicts={} restarts={} heap_rebuilds={}",
                    stats.decisions, stats.conflicts, stats.restarts, stats.heap_rebuilds
                );
                last_report = stats.conflicts;
            }
            // Propagate
            match self.propagate() {
                Ok(()) => {
                    // No conflict - pick next literal or return SAT
                    let (var, rebuilds) = self.pick_branching_literal_tracked();
                    stats.heap_rebuilds += rebuilds;
                    match var {
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
