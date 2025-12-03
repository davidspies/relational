//! CauseSink - accumulates causes with deterministic ordering via seeded hash.

use std::hash::{Hash, Hasher};

use contiguous_data::{L2Heaps, L2Multiset, Multiset};
use relational::database::{CommitId, Sink};

use super::types::{ClauseId, Level, Lit};

/// Seeded hash for deterministic ordering.
fn seeded_hash<T: Hash>(val: &T, seed: u64) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    seed.hash(&mut hasher);
    val.hash(&mut hasher);
    hasher.finish()
}

/// A sink that accumulates cause information for conflict analysis.
///
/// Uses L2Heaps with seeded hash for deterministic ordering when multiple
/// causes exist for the same literal.
pub struct CauseSink {
    /// Maps lit -> min-heap of (hash, clause_id)
    data: L2Heaps<Lit, (u64, ClauseId)>,
    /// Tracks multiplicity for data entries
    data_counts: Multiset<(Lit, u64, ClauseId)>,
    assigned_at: L2Multiset<Lit, (CommitId, Level)>,
    seed: u64,
}

impl Default for CauseSink {
    fn default() -> Self {
        Self {
            data: L2Heaps::new(),
            data_counts: Multiset::new(),
            assigned_at: L2Multiset::new(),
            seed: 0x7a3d9f1e4b2c8a05, // arbitrary fixed seed
        }
    }
}

impl CauseSink {
    /// Get the reason clause for a literal.
    pub fn get_reason(&self, lit: Lit) -> Option<ClauseId> {
        let &(_, clause) = self.data.peek(&lit)?;
        (!clause.is_decision()).then_some(clause)
    }

    /// Get the earliest commit ID for a literal.
    pub fn get_commit_id(&self, lit: Lit) -> Option<CommitId> {
        let &(commits, _) = self.assigned_at.get_singleton(&lit)?;
        Some(commits)
    }

    pub fn get_level(&self, lit: Lit) -> Option<Level> {
        let &(_, level) = self.assigned_at.get_singleton(&lit)?;
        Some(level)
    }

    pub fn contains_lit(&self, lit: Lit) -> bool {
        !self.data.is_empty(&lit)
    }
}

impl Sink<((Lit, CommitId), (ClauseId, Level))> for CauseSink {
    fn dump_all(&mut self, incoming: &mut Multiset<((Lit, CommitId), (ClauseId, Level))>) {
        for (((lit, commit_id), (clause_id, level)), diff) in incoming.drain() {
            let hash = seeded_hash(&clause_id, self.seed);
            let count_key = (lit, hash, clause_id);
            let old_count = self.data_counts.get(&count_key);
            self.data_counts.update(count_key, diff);
            let new_count = self.data_counts.get(&count_key);

            // Add to heap when count goes from 0 to non-zero
            if old_count == 0 && new_count != 0 {
                self.data.push(lit, (hash, clause_id));
            }
            // Remove from heap when count goes from non-zero to 0
            if old_count != 0 && new_count == 0 {
                self.data.remove(&lit, &(hash, clause_id));
            }

            self.assigned_at.update(lit, (commit_id, level), diff);
        }
    }
}
