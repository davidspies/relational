//! Constraint deletion strategies for CDCL solver.
//!
//! Implements LBD (Literal Block Distance) based constraint management.
//! LBD is the number of distinct decision levels in a constraint - lower is better.

use contiguous_data::HashMap;

use crate::types::{ConstraintId, Level, Lit};

/// Metadata tracked for each learned constraint.
#[derive(Clone, Copy)]
struct ConstraintInfo {
    /// LBD (Literal Block Distance) - number of distinct decision levels.
    lbd: u32,
    /// Activity score - bumped when constraint is used in conflict analysis.
    activity: f64,
}

/// Manages learned constraint deletion.
pub(crate) struct ClauseDeletion {
    /// Metadata for each learned constraint.
    constraint_info: HashMap<ConstraintId, ConstraintInfo>,
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
    /// Create a new constraint deletion manager.
    pub(crate) fn new() -> Self {
        Self {
            constraint_info: HashMap::default(),
            max_clauses: 2000,
            growth_factor: 1.1,
            activity_decay: 0.95,
            activity_inc: 1.0,
            glue_threshold: 2,
        }
    }

    /// Register a newly learned constraint with its LBD.
    pub(crate) fn on_learn(&mut self, cid: ConstraintId, clause: &[(Lit, Level)]) {
        let lbd = compute_lbd(clause);
        self.constraint_info.insert(
            cid,
            ConstraintInfo {
                lbd,
                activity: self.activity_inc,
            },
        );
    }

    /// Bump activity for a constraint used during conflict analysis.
    pub(crate) fn bump_activity(&mut self, cid: ConstraintId) {
        if let Some(info) = self.constraint_info.get_mut(&cid) {
            info.activity += self.activity_inc;
        }
    }

    /// Decay all constraint activities (call after each conflict).
    pub(crate) fn decay_activities(&mut self) {
        // Instead of decaying all, just increase the increment.
        // This is equivalent but avoids iterating all constraints.
        self.activity_inc /= self.activity_decay;
    }

    /// Check if constraint deletion should be triggered.
    pub(crate) fn should_delete(&self) -> bool {
        self.constraint_info.len() > self.max_clauses
    }

    /// Select constraints to delete. Returns constraint IDs to remove.
    ///
    /// Protects "glue" constraints (low LBD) and removes half of the rest,
    /// prioritizing low-activity constraints.
    pub(crate) fn select_for_deletion(&mut self) -> Vec<ConstraintId> {
        // Collect non-glue constraints
        let mut candidates: Vec<_> = self
            .constraint_info
            .iter()
            .filter(|(_, info)| info.lbd > self.glue_threshold)
            .map(|(&cid, info)| (cid, info.activity))
            .collect();

        // Sort by activity (ascending - delete lowest activity first)
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        // Delete half of non-protected constraints
        let to_delete = candidates.len() / 2;
        let deleted: Vec<_> = candidates
            .into_iter()
            .take(to_delete)
            .map(|(cid, _)| cid)
            .collect();

        // Remove from our tracking
        for &cid in &deleted {
            self.constraint_info.remove(&cid);
        }

        // Grow the limit for next time
        self.max_clauses = (self.max_clauses as f64 * self.growth_factor) as usize;

        deleted
    }

    /// Number of learned constraints currently tracked.
    pub(crate) fn len(&self) -> usize {
        self.constraint_info.len()
    }
}

impl Default for ClauseDeletion {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the LBD (Literal Block Distance) of a clause.
/// LBD is the number of distinct decision levels among the clause's literals.
fn compute_lbd(clause: &[(Lit, Level)]) -> u32 {
    // Use a small vec for typical clause sizes, avoiding allocation
    let mut seen_levels: Vec<Level> = Vec::with_capacity(clause.len());
    for &(_, level) in clause {
        if !seen_levels.contains(&level) {
            seen_levels.push(level);
        }
    }
    seen_levels.len() as u32
}

#[cfg(test)]
mod tests;
