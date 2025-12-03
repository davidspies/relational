//! Conflict analysis for CDCL: 1-UIP clause learning and backtrack level computation.
//!
//! When a conflict occurs, we analyze the implication graph to learn a clause
//! that prevents the same conflict from recurring. The standard approach is:
//!
//! 1. Start with the conflicting clause (or both conflicting literals)
//! 2. Resolve backward through the implication graph until we reach a 1-UIP
//!    (exactly one literal at the current decision level in the learned clause)
//! 3. The backtrack level is the second-highest level among literals in the clause

mod working_set;

use working_set::WorkingSet;

use crate::Solver;
use crate::types::{Conflict, Level, Lit};

/// Result of conflict analysis.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// The learned clause (negations of the literals that led to conflict).
    pub learned_clause: Vec<Lit>,
    /// The level to backtrack to (second-highest level in learned clause).
    pub backtrack_level: Level,
}

impl Solver {
    /// Analyze a conflict and return the learned clause and backtrack level.
    ///
    /// Uses the 1-UIP (First Unique Implication Point) scheme:
    /// - Start from conflict clause/literals
    /// - Resolve backward until exactly one literal at current level remains
    /// - That literal is the UIP - it's the "decision" that forced this conflict
    pub fn analyze_conflict(&self, conflict: Conflict) -> Option<AnalysisResult> {
        if self.state.current_level == Level::TOP {
            // Conflict at level 0 means UNSAT - nothing to learn
            return None;
        }

        // Get the implication graph and assignments
        let causes = self.outputs.causes.get();
        let assignments = self.outputs.assignments.get();

        // Initialize the working set (nogood): the true assignments that caused conflict
        let initial_lits: Vec<Lit> = match conflict {
            Conflict::EmptyClause(cid) => {
                // All literals in this clause are false - the nogood is their negations
                self.get_clause(cid)
                    .expect("conflict clause must exist")
                    .iter()
                    .map(|&lit| !lit)
                    .collect()
            }
            Conflict::DirectConflict(var) => {
                // Both lit and !lit are assigned - include both
                vec![Lit::pos(var), Lit::neg(var)]
            }
        };

        // Use the minimum of current level and max level in conflict literals
        let max_conflict_level = initial_lits
            .iter()
            .map(|lit| *assignments.get_singleton(lit))
            .max()
            .unwrap_or(Level::TOP);
        let current_level = self.state.current_level.min(max_conflict_level);

        if current_level == Level::TOP {
            return None;
        }

        let mut working = WorkingSet::new();
        for lit in initial_lits {
            working.insert(lit, current_level, &assignments, &causes);
        }

        // Resolution loop: resolve until we have exactly 1 literal at current level (1-UIP)
        while working.count_at_current() > 1 {
            // Pop the most recently assigned literal at current level
            let Some((_, lit)) = working.pop_most_recent() else {
                // No resolvable literal found - shouldn't happen in valid state
                break;
            };

            // Get the reason clause for this literal
            let Some(reason_cid) = causes.get_reason(lit) else {
                // This was a decision, can't resolve further
                break;
            };

            // Resolve: add the negations of other clause literals
            let clause_lits = self
                .get_clause(reason_cid)
                .expect("reason clause must exist");
            for &clause_lit in clause_lits {
                // Skip the literal we're resolving on
                if clause_lit != lit {
                    // Add the negated literal (the true assignment that made this false)
                    working.insert(!clause_lit, current_level, &assignments, &causes);
                }
            }
        }

        // Build the learned clause: negate each literal in working set
        // (working contains literals that are true and led to conflict,
        // learned clause contains their negations to prevent this)
        let learned_clause: Vec<Lit> = working.iter().map(|lit| !lit).collect();

        if learned_clause.is_empty() {
            return None;
        }

        // Find backtrack level: second-highest level among learned clause literals
        let mut levels: Vec<Level> = learned_clause
            .iter()
            .map(|&lit| *assignments.get_singleton(&(!lit)))
            .collect();
        levels.sort();
        levels.dedup();

        let backtrack_level = if levels.len() <= 1 {
            Level::TOP
        } else {
            levels[levels.len() - 2]
        };

        Some(AnalysisResult {
            learned_clause,
            backtrack_level,
        })
    }
}
