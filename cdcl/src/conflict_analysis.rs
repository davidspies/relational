//! Conflict analysis using the relational layer.

use relational::database::Database;

use crate::Solver;
use crate::types::{Conflict, Level, Lit};

/// Result of conflict analysis.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// The learned clause with levels: (literal, level) pairs.
    pub learned_clause: Vec<(Lit, Level)>,
    /// The max level of any literal in the learned clause.
    pub conflict_level: Level,
    /// The second highest level (backtrack target for non-chronological backtracking).
    pub backtrack_level: Level,
}

impl Solver {
    /// Analyze a conflict using the relational layer and return the learned clause.
    ///
    /// Uses 1-UIP conflict analysis computed via dataflow:
    /// 1. Push a new frame
    /// 2. Feed the conflict into analysis_inp
    /// 3. Commit (runs the analysis dataflow to fixpoint)
    /// 4. Read out the learned clause from new_clause output
    /// 5. Pop the frame
    /// 6. Return the result
    pub(crate) fn analyze_conflict(
        &mut self,
        db: &mut Database,
        conflict: Conflict,
    ) -> AnalysisResult {
        // 1. Push a new frame
        db.push();

        // 2. Feed the conflict into analysis_inp
        self.inputs.analysis.insert(conflict);

        // 3. Commit (runs analysis dataflow)
        db.commit();

        // 4. Read out the learned clause with levels
        let mut learned_clause: Vec<(Lit, Level)> =
            self.outputs.new_clause.get().iter().copied().collect();
        learned_clause.sort();

        // Bump activity for clauses that participated in the analysis
        for clause_id in self.outputs.analysis_clause_ids.get().iter() {
            self.state.clause_deletion.bump_activity(*clause_id);
        }

        // Get the conflict level (max level in learned clause)
        let conflict_level = learned_clause
            .iter()
            .map(|&(_lit, level)| level)
            .max()
            .unwrap_or(Level::TOP);

        // Get the second highest level (backtrack target)
        let backtrack_level = learned_clause
            .iter()
            .map(|&(_lit, level)| level)
            .filter(|&level| level != conflict_level)
            .max()
            .unwrap_or(Level::TOP);

        // 5. Pop the frame
        let popped = db.pop();
        assert!(popped, "Analysis frame should exist");

        AnalysisResult {
            learned_clause,
            conflict_level,
            backtrack_level,
        }
    }
}
