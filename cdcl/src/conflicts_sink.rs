//! ConflictsSink - tracks conflicts with deterministic ordering via seeded hash.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use contiguous_data::Multiset;
use relational::database::Sink;

use super::types::Conflict;

/// Seeded hash for deterministic ordering.
fn seeded_hash<T: Hash>(val: &T, seed: u64) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    seed.hash(&mut hasher);
    val.hash(&mut hasher);
    hasher.finish()
}

/// A sink that tracks conflicts with deterministic ordering.
///
/// Uses BTreeMap with seeded hash so that when multiple conflicts exist,
/// we pick the same one deterministically across runs.
#[derive(Clone)]
pub struct ConflictsSink {
    /// Maps (hash, conflict) -> multiplicity
    data: BTreeMap<(u64, Conflict), i64>,
    seed: u64,
}

impl Default for ConflictsSink {
    fn default() -> Self {
        Self {
            data: BTreeMap::new(),
            seed: 0x3e8a1f5d9c2b7046, // arbitrary fixed seed
        }
    }
}

impl ConflictsSink {
    /// Get the first conflict (deterministic ordering via seeded hash).
    pub fn first(&self) -> Option<Conflict> {
        self.data.keys().next().map(|(_, c)| *c)
    }
}

impl Sink<Conflict> for ConflictsSink {
    fn dump_all(&mut self, incoming: &mut Multiset<Conflict>) {
        for (conflict, diff) in incoming.drain() {
            let hash = seeded_hash(&conflict, self.seed);
            let key = (hash, conflict);
            let entry = self.data.entry(key).or_insert(0);
            *entry += diff;
            if *entry == 0 {
                self.data.remove(&key);
            }
        }
    }
}
