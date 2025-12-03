//! LiteralCountsSink - tracks literal counts with seeded hash for deterministic ordering.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use contiguous_data::Multiset;
use relational::database::Sink;

use super::types::Lit;

/// Seeded hash of a literal for deterministic but randomizable ordering.
fn seeded_hash(lit: Lit, seed: u64) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    seed.hash(&mut hasher);
    lit.hash(&mut hasher);
    hasher.finish()
}

/// A sink that tracks literals by (count, seeded_hash, lit) for deterministic ordering.
///
/// The BTreeMap key ordering gives us:
/// 1. Literals grouped by count (ascending)
/// 2. Within same count, ordered by seeded hash (deterministic randomness)
/// 3. Lit for uniqueness
#[derive(Clone)]
pub struct LiteralCountsSink {
    /// Maps (count, hash, lit) -> multiplicity
    data: BTreeMap<(i64, u64, Lit), i64>,
    /// Seed for hash function (controls tie-breaking order)
    seed: u64,
}

impl Default for LiteralCountsSink {
    fn default() -> Self {
        // Use a fixed seed for now; could be made configurable
        Self {
            data: BTreeMap::new(),
            seed: 0x517cc1b727220a95, // arbitrary fixed seed
        }
    }
}

impl LiteralCountsSink {
    /// Get the literal with highest count (deterministic tie-breaking via seeded hash).
    pub fn max_count(&self) -> Option<(i64, Lit)> {
        self.data.last_key_value().map(|(&(count, _, lit), _)| (count, lit))
    }
}

impl Sink<(Lit, i64)> for LiteralCountsSink {
    fn dump_all(&mut self, incoming: &mut Multiset<(Lit, i64)>) {
        for ((lit, count), diff) in incoming.drain() {
            let hash = seeded_hash(lit, self.seed);
            let key = (count, hash, lit);
            let entry = self.data.entry(key).or_insert(0);
            *entry += diff;
            if *entry == 0 {
                self.data.remove(&key);
            }
        }
    }
}
