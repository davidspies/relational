//! CauseSink - accumulates causes with deterministic ordering via seeded hash.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use contiguous_data::{L2Multiset, Multiset};
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
/// Uses BTreeMap with seeded hash for deterministic ordering when multiple
/// causes exist for the same literal/commit.
pub struct CauseSink {
    /// Maps lit -> (hash, clause_id) -> multiplicity
    data: HashMap<Lit, BTreeMap<(u64, ClauseId), i64>>,
    assigned_at: L2Multiset<Lit, (CommitId, Level)>,
    seed: u64,
}

impl Default for CauseSink {
    fn default() -> Self {
        Self {
            data: HashMap::new(),
            assigned_at: L2Multiset::new(),
            seed: 0x7a3d9f1e4b2c8a05, // arbitrary fixed seed
        }
    }
}

impl CauseSink {
    /// Get the reason clause for a literal.
    pub fn get_reason(&self, lit: Lit) -> Option<ClauseId> {
        let commits = self.data.get(&lit)?;
        let &(_, clause) = commits.keys().next().unwrap();
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
        self.data.contains_key(&lit)
    }
}

impl Sink<((Lit, CommitId), (ClauseId, Level))> for CauseSink {
    fn dump_all(&mut self, incoming: &mut Multiset<((Lit, CommitId), (ClauseId, Level))>) {
        for (((lit, commit_id), (clause_id, level)), diff) in incoming.drain() {
            let commits = self.data.entry(lit).or_default();
            let hash = seeded_hash(&clause_id, self.seed);
            let key = (hash, clause_id);
            let entry = commits.entry(key).or_insert(0);
            *entry += diff;
            if *entry == 0 {
                commits.remove(&key);
                if commits.is_empty() {
                    self.data.remove(&lit);
                }
            }
            self.assigned_at.update(lit, (commit_id, level), diff);
        }
    }
}
