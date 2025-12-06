//! Main solve loop for the CDCL solver.

use std::time::Instant;

use relational::database::Database;

use crate::conflict_analysis::AnalysisResult;
use crate::types::Level;

use super::Solver;
use super::proof::ProofWriter;

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

                    if conflict_level == Level::TOP {
                        // Conflict at level 0 = UNSAT
                        if let Some(ref mut p) = proof {
                            let _ = p.flush();
                        }
                        return (false, stats);
                    };

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
