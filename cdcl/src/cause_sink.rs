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
/// Uses L2Heaps ordered by (CommitId, hash, ClauseId) so we get the earliest
/// derivation first, with deterministic tie-breaking via hash.
pub struct CauseSink {
    /// Maps lit -> min-heap of (commit_id, hash, clause_id)
    data: L2Heaps<Lit, (CommitId, u64, ClauseId)>,
    /// Tracks multiplicity for data entries
    data_counts: Multiset<(Lit, CommitId, u64, ClauseId)>,
    /// Maps lit -> level (should be singleton per lit)
    levels: L2Multiset<Lit, Level>,
    seed: u64,
}

impl Default for CauseSink {
    fn default() -> Self {
        Self {
            data: L2Heaps::new(),
            data_counts: Multiset::new(),
            levels: L2Multiset::new(),
            seed: 0x7a3d9f1e4b2c8a05, // arbitrary fixed seed
        }
    }
}

impl CauseSink {
    /// Get the reason clause for a literal (from the earliest derivation).
    pub fn get_reason(&self, lit: Lit) -> Option<ClauseId> {
        let &(_, _, clause) = self.data.peek(&lit)?;
        (!clause.is_decision()).then_some(clause)
    }

    /// Get the earliest commit ID for a literal.
    pub fn get_commit_id(&self, lit: Lit) -> Option<CommitId> {
        let &(commit_id, _, _) = self.data.peek(&lit)?;
        Some(commit_id)
    }

    pub fn get_level(&self, lit: Lit) -> Option<Level> {
        self.levels.get_singleton(&lit).copied()
    }

    pub fn contains_lit(&self, lit: Lit) -> bool {
        !self.data.is_empty(&lit)
    }

    /// Count how many literals are assigned (debug).
    pub fn count_assigned(&self) -> usize {
        self.levels.keys().count()
    }

    /// Count literals assigned at a specific level.
    pub fn count_at_level(&self, level: Level) -> usize {
        self.levels.keys().filter(|&lit| self.levels.get_singleton(lit) == Some(&level)).count()
    }

    /// Count literals not assigned at level 0 (non-fixed assignments).
    pub fn count_non_fixed(&self) -> usize {
        self.levels
            .keys()
            .filter(|&lit| self.levels.get_singleton(lit) != Some(&Level::TOP))
            .count()
    }
}

impl Sink<((Lit, CommitId), (ClauseId, Level))> for CauseSink {
    fn dump_all(&mut self, incoming: &mut Multiset<((Lit, CommitId), (ClauseId, Level))>) {
        for (((lit, commit_id), (clause_id, level)), diff) in incoming.drain() {
            let hash = seeded_hash(&clause_id, self.seed);
            let count_key = (lit, commit_id, hash, clause_id);
            let old_count = self.data_counts.get(&count_key);
            self.data_counts.update(count_key, diff);
            let new_count = self.data_counts.get(&count_key);

            // Add to heap when count goes from 0 to non-zero
            if old_count == 0 && new_count != 0 {
                self.data.push(lit, (commit_id, hash, clause_id));
            }
            // Remove from heap when count goes from non-zero to 0
            if old_count != 0 && new_count == 0 {
                self.data.remove(&lit, &(commit_id, hash, clause_id));
            }

            self.levels.update(lit, level, diff);
        }
    }
}
