//! Clause deletion strategies for CDCL solver.
//!
//! Implements LBD (Literal Block Distance) based clause management.
//! LBD is the number of distinct decision levels in a clause - lower is better.

use ahash::AHashMap;

use crate::types::{ClauseId, Level, Lit};

/// Metadata tracked for each learned clause.
#[derive(Clone, Copy)]
struct ClauseInfo {
    /// LBD (Literal Block Distance) - number of distinct decision levels.
    lbd: u32,
    /// Activity score - bumped when clause is used in conflict analysis.
    activity: f64,
}

/// Manages learned clause deletion.
pub(crate) struct ClauseDeletion {
    /// Metadata for each learned clause.
    clause_info: AHashMap<ClauseId, ClauseInfo>,
    /// Maximum number of learned clauses before triggering deletion.
    max_clauses: usize,
    /// How much to grow max_clauses after each deletion.
    growth_factor: f64,
    /// Activity decay factor (applied after each conflict).
    activity_decay: f64,
    /// Activity increment (applied when clause is used).
    activity_inc: f64,
    /// LBD threshold - clauses with LBD <= this are "glue" and protected.
    glue_threshold: u32,
}

impl ClauseDeletion {
    /// Create a new clause deletion manager.
    pub(crate) fn new() -> Self {
        Self {
            clause_info: AHashMap::new(),
            max_clauses: 2000,
            growth_factor: 1.1,
            activity_decay: 0.95,
            activity_inc: 1.0,
            glue_threshold: 2,
        }
    }

    /// Register a newly learned clause with its LBD.
    pub(crate) fn on_learn(&mut self, clause_id: ClauseId, literals: &[Lit], levels: &[Level]) {
        let lbd = compute_lbd(literals, levels);
        self.clause_info.insert(
            clause_id,
            ClauseInfo {
                lbd,
                activity: self.activity_inc,
            },
        );
    }

    /// Bump activity for a clause used during conflict analysis.
    #[allow(dead_code)]
    pub(crate) fn bump_activity(&mut self, clause_id: ClauseId) {
        if let Some(info) = self.clause_info.get_mut(&clause_id) {
            info.activity += self.activity_inc;
        }
    }

    /// Decay all clause activities (call after each conflict).
    pub(crate) fn decay_activities(&mut self) {
        // Instead of decaying all, just increase the increment.
        // This is equivalent but avoids iterating all clauses.
        self.activity_inc /= self.activity_decay;
    }

    /// Check if clause deletion should be triggered.
    pub(crate) fn should_delete(&self) -> bool {
        self.clause_info.len() > self.max_clauses
    }

    /// Select clauses to delete. Returns clause IDs to remove.
    ///
    /// Protects "glue" clauses (low LBD) and removes half of the rest,
    /// prioritizing low-activity clauses.
    pub(crate) fn select_for_deletion(&mut self) -> Vec<ClauseId> {
        // Collect non-glue clauses
        let mut candidates: Vec<_> = self
            .clause_info
            .iter()
            .filter(|(_, info)| info.lbd > self.glue_threshold)
            .map(|(&cid, info)| (cid, info.activity))
            .collect();

        // Sort by activity (ascending - delete lowest activity first)
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        // Delete half of non-protected clauses
        let to_delete = candidates.len() / 2;
        let deleted: Vec<_> = candidates
            .into_iter()
            .take(to_delete)
            .map(|(cid, _)| cid)
            .collect();

        // Remove from our tracking
        for &cid in &deleted {
            self.clause_info.remove(&cid);
        }

        // Grow the limit for next time
        self.max_clauses = (self.max_clauses as f64 * self.growth_factor) as usize;

        deleted
    }

    /// Number of learned clauses currently tracked.
    pub(crate) fn len(&self) -> usize {
        self.clause_info.len()
    }
}

impl Default for ClauseDeletion {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the LBD (Literal Block Distance) of a clause.
/// LBD is the number of distinct decision levels among the clause's literals.
fn compute_lbd(literals: &[Lit], levels: &[Level]) -> u32 {
    assert_eq!(literals.len(), levels.len());

    // Use a small vec for typical clause sizes, avoiding allocation
    let mut seen_levels: Vec<Level> = Vec::with_capacity(literals.len());
    for &level in levels {
        if !seen_levels.contains(&level) {
            seen_levels.push(level);
        }
    }
    seen_levels.len() as u32
}

#[cfg(test)]
mod tests;
