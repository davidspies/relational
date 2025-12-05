//! ConflictsSink - tracks conflicts with deterministic ordering via seeded hash.

use std::hash::Hash;

use ahash::RandomState;
use contiguous_data::Multiset;
use priority_queue::PriorityQueue;
use relational::database::Sink;

use super::types::Conflict;

/// Seeded hash for deterministic ordering.
fn seeded_hash<T: Hash>(val: &T, seed: u64) -> u64 {
    let build_hasher = RandomState::with_seeds(seed, 0, 0, 0);
    build_hasher.hash_one(val)
}

/// A sink that tracks conflicts with deterministic ordering.
///
/// Uses PriorityQueue + Multiset. The priority queue gives O(1) access to the
/// highest-hash conflict, while the multiset tracks multiplicities.
#[derive(Clone)]
pub(crate) struct ConflictsSink {
    /// Priority queue: conflict -> hash for deterministic ordering
    queue: PriorityQueue<Conflict, (u64, Conflict)>,
    /// Tracks multiplicities
    counts: Multiset<Conflict>,
    seed: u64,
}

impl Default for ConflictsSink {
    fn default() -> Self {
        Self {
            queue: PriorityQueue::new(),
            counts: Multiset::new(),
            seed: 0x3e8a1f5d9c2b7046, // arbitrary fixed seed
        }
    }
}

impl ConflictsSink {
    /// Get the first conflict (deterministic ordering via seeded hash).
    pub(crate) fn first(&self) -> Option<Conflict> {
        self.queue.peek().map(|(&c, _)| c)
    }
}

impl Sink<Conflict> for ConflictsSink {
    fn dump_all(&mut self, incoming: &mut Multiset<Conflict>) {
        for (conflict, diff) in incoming.drain() {
            let was_present = self.counts.contains(&conflict);
            self.counts.update(conflict, diff);
            let is_present = self.counts.contains(&conflict);

            if !was_present && is_present {
                let hash = seeded_hash(&conflict, self.seed);
                self.queue.push(conflict, (hash, conflict));
            } else if was_present && !is_present {
                self.queue.remove(&conflict);
            }
        }
    }
}
