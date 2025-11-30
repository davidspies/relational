//! Conflict analysis for CDCL: 1-UIP clause learning and backtrack level computation.
//!
//! When a conflict occurs, we analyze the implication graph to learn a clause
//! that prevents the same conflict from recurring. The standard approach is:
//!
//! 1. Start with the conflicting clause (or both conflicting literals)
//! 2. Resolve backward through the implication graph until we reach a 1-UIP
//!    (exactly one literal at the current decision level in the learned clause)
//! 3. The backtrack level is the second-highest level among literals in the clause

use std::collections::HashSet;

use super::Solver;
use super::assignments_sink::AssignmentsSink;
use super::cause_sink::CauseSink;
use super::types::{Conflict, Level, Lit};

/// Result of conflict analysis.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// The learned clause (negations of the literals that led to conflict).
    pub learned_clause: Vec<Lit>,
    /// The level to backtrack to (second-highest level in learned clause).
    pub backtrack_level: Level,
}

/// Count literals at the given decision level.
fn count_at_level(working: &HashSet<Lit>, assignments: &AssignmentsSink, level: Level) -> usize {
    working
        .iter()
        .filter(|lit| assignments.get(lit).unwrap_or(Level::TOP) == level)
        .count()
}

impl Solver {
    /// Analyze a conflict and return the learned clause and backtrack level.
    ///
    /// Uses the 1-UIP (First Unique Implication Point) scheme:
    /// - Start from conflict clause/literals
    /// - Resolve backward until exactly one literal at current level remains
    /// - That literal is the UIP - it's the "decision" that forced this conflict
    pub fn analyze_conflict(&self, conflict: Conflict) -> Option<AnalysisResult> {
        let current_level = self.state.current_level;

        if current_level == Level::TOP {
            // Conflict at level 0 means UNSAT - nothing to learn
            return None;
        }

        // Get the implication graph and assignments
        let causes = self.outputs.causes.get();
        let assignments = self.outputs.assignments.get();

        // Initialize the working set (nogood): the true assignments that caused conflict
        let mut working: HashSet<Lit> = match conflict {
            Conflict::EmptyClause(cid) => {
                // All literals in this clause are false - the nogood is their negations
                self.get_clause(cid)
                    .expect("conflict clause must exist")
                    .iter()
                    .map(|lit| lit.negated())
                    .collect()
            }
            Conflict::DirectConflict(var) => {
                // Both lit and !lit are assigned - include both
                HashSet::from([Lit::pos(var), Lit::neg(var)])
            }
        };

        // Resolution loop: resolve until we have exactly 1 literal at current level (1-UIP)
        while count_at_level(&working, &assignments, current_level) > 1 {
            // Find the most recently assigned literal at current level that has a reason
            let resolve_lit =
                find_most_recent_at_level(&working, &causes, &assignments, current_level);

            let Some(lit) = resolve_lit else {
                // No resolvable literal found - shouldn't happen in valid state
                break;
            };

            // Get the reason clause for this literal
            let Some(reason_cid) = causes.get_reason(lit) else {
                // This was a decision, can't resolve further
                break;
            };

            // Resolve: remove lit, add the negations of other clause literals
            working.remove(&lit);
            let clause_lits = self
                .get_clause(reason_cid)
                .expect("reason clause must exist");
            for &clause_lit in clause_lits {
                // Skip the literal we're resolving on
                if clause_lit != lit {
                    // Add the negated literal (the true assignment that made this false)
                    working.insert(clause_lit.negated());
                }
            }
        }

        // Build the learned clause: negate each literal in working set
        // (working contains literals that are true and led to conflict,
        // learned clause contains their negations to prevent this)
        let learned_clause: Vec<Lit> = working.iter().map(|lit| lit.negated()).collect();

        if learned_clause.is_empty() {
            return None;
        }

        // Find backtrack level: second-highest level among learned clause literals
        let mut levels: Vec<Level> = learned_clause
            .iter()
            .filter_map(|lit| assignments.get(&lit.negated()))
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

/// Find the most recently assigned literal at the given level.
fn find_most_recent_at_level(
    working: &HashSet<Lit>,
    causes: &CauseSink,
    assignments: &AssignmentsSink,
    current_level: Level,
) -> Option<Lit> {
    // Get literals at current level that have reasons (not decisions)
    let mut candidates: Vec<_> = working
        .iter()
        .filter(|lit| assignments.get(lit) == Some(current_level))
        .filter_map(|&lit| causes.get_commit_id(lit).map(|cid| (lit, cid)))
        .collect();

    // Sort by commit ID descending (most recent first)
    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    candidates.first().map(|(lit, _)| *lit)
}
