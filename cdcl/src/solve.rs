//! Main solve loop for the CDCL solver.

use std::time::Instant;

use relational::database::Database;

use super::Solver;
use super::proof::ProofWriter;
use super::types::{ClauseId, Level};

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
    pub fn solve_with_proof(
        &mut self,
        db: &mut Database,
        mut proof: Option<&mut ProofWriter>,
    ) -> bool {
        // Ensure fixpoint runs to detect unary literals from original clauses
        db.commit();

        // Check for contradictions from unary clause simplification at level 0
        if !self.outputs.l0_contradiction.get().is_empty() {
            // UNSAT due to unit propagation before any decisions
            if let Some(ref mut p) = proof {
                let _ = p.add_empty_clause();
                let _ = p.flush();
            }
            eprintln!("c stats: 0.000s decisions=0 conflicts=0 restarts=0");
            return false;
        }

        // Process unary literals from binary contradictions in a loop
        loop {
            let binary_unary_lits: Vec<_> = self
                .outputs
                .unary_from_binary_contradiction
                .get()
                .iter()
                .copied()
                .collect();
            if binary_unary_lits.is_empty() {
                break;
            }

            // Learn each as a singleton clause and add to proof
            for lit in binary_unary_lits {
                self.learn_clause(db, &[lit]);
                self.state.binary_unary_count += 1;
                if let Some(ref mut p) = proof {
                    let _ = p.add_clause(&[lit]);
                }
            }
        }

        // Process and assign unary literals (from original clauses)
        if self.process_new_unary_literals(db) {
            // UNSAT due to unary literal contradiction
            if let Some(ref mut p) = proof {
                let _ = p.add_empty_clause();
                let _ = p.flush();
            }
            eprintln!("c stats: 0.000s decisions=0 conflicts=0 restarts=0");
            return false;
        }

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

    /// Check for new unary literals, assign them at level TOP, and remove their variables from VSIDS.
    /// Returns true if a level 0 contradiction is detected (UNSAT).
    fn process_new_unary_literals(&mut self, db: &mut Database) -> bool {
        let unary_lits: Vec<_> = self.outputs.unary_lits.get().iter().copied().collect();

        // Assign all unary literals at level TOP (level 0)
        for lit in &unary_lits {
            self.inputs
                .decision_assignments
                .insert((*lit, Level::TOP, ClauseId::DECISION));
        }

        // Commit to propagate these assignments
        if !unary_lits.is_empty() {
            db.commit();

            // Check for contradictions after assigning unary literals
            if !self.outputs.l0_contradiction.get().is_empty() {
                return true;
            }
        }

        // Remove unary variables from VSIDS
        for lit in unary_lits {
            self.state.vsids.remove(lit.var());
        }

        false
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
                let binary_unary = self.state.binary_unary_count;
                let elapsed = start.elapsed().as_secs_f64();
                let SolveStats {
                    decisions,
                    conflicts,
                    restarts,
                } = stats;
                eprintln!(
                    "c progress: {elapsed:.1}s decisions={decisions} conflicts={conflicts} restarts={restarts} \
                    learned={learned} binary_unary={binary_unary} level={level} assigned={assigned_non_fixed}/{total_non_fixed}",
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

                            // Process unary literals from binary contradictions in a loop
                            loop {
                                let binary_unary_lits: Vec<_> = self
                                    .outputs
                                    .unary_from_binary_contradiction
                                    .get()
                                    .iter()
                                    .copied()
                                    .collect();
                                if binary_unary_lits.is_empty() {
                                    break;
                                }

                                // Learn each as a singleton clause and add to proof
                                for lit in binary_unary_lits {
                                    self.learn_clause(db, &[lit]);
                                    self.state.binary_unary_count += 1;
                                    if let Some(ref mut p) = proof {
                                        let _ = p.add_clause(&[lit]);
                                    }
                                }
                            }

                            // Check for new unary literals (can appear after learning unary/binary clauses)
                            if self.process_new_unary_literals(db) {
                                // UNSAT due to unary literal contradiction
                                if let Some(ref mut p) = proof {
                                    let _ = p.add_empty_clause();
                                    let _ = p.flush();
                                }
                                return (false, stats);
                            }

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
