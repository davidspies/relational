//! VSIDS (Variable State Independent Decaying Sum) decision heuristic.
//!
//! Uses a priority queue for O(log n) pick and O(log n) bump.

use std::cmp::Ordering;

use contiguous_data::{HashMap, HashSet};
use priority_queue::PriorityQueue;
use rand_chacha::{
    ChaCha8Rng,
    rand_core::{Rng, SeedableRng},
};

use super::types::Var;

/// Priority: (activity, nonce). Higher activity wins; nonce breaks ties randomly.
#[derive(Clone, Copy, PartialEq)]
struct Priority {
    activity: f64,
    nonce: u64,
}

impl Eq for Priority {}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.activity.partial_cmp(&other.activity) {
            Some(Ordering::Equal) | None => self.nonce.cmp(&other.nonce),
            Some(ord) => ord,
        }
    }
}

pub(crate) struct Vsids {
    /// Priority queue: var -> priority (only unassigned variables)
    queue: PriorityQueue<Var, Priority>,
    /// Stashed priorities for assigned variables (removed from queue)
    stashed: HashMap<Var, Priority>,
    /// Bump amount (increases for decay effect)
    bump: f64,
    /// Saved phase per variable
    phase: HashMap<Var, bool>,
    /// RNG for generating tie-breaking nonces
    rng: ChaCha8Rng,
}

impl Vsids {
    pub(crate) fn new(vars: &HashSet<Var>) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(0x5a7d3e1f9c2b8a04);
        let mut queue = PriorityQueue::with_capacity(vars.len());
        // Initialize all variables with 0 activity and random nonces
        let mut sorted_vars: Vec<Var> = vars.iter().copied().collect();
        sorted_vars.sort();
        for &var in &sorted_vars {
            queue.push(
                var,
                Priority {
                    activity: 0.0,
                    nonce: rng.next_u64(),
                },
            );
        }
        Self {
            queue,
            stashed: HashMap::default(),
            bump: 1.0,
            phase: HashMap::default(),
            rng,
        }
    }

    pub(crate) fn bump(&mut self, var: Var) {
        let bump = self.bump;
        // Try queue first, fall back to stashed
        if self.queue.get(&var).is_some() {
            self.queue.change_priority_by(&var, |p| p.activity += bump);
        } else if let Some(priority) = self.stashed.get_mut(&var) {
            priority.activity += bump;
        }
    }

    pub(crate) fn decay(&mut self) {
        self.bump *= 1.05;
    }

    pub(crate) fn set_phase(&mut self, var: Var, positive: bool) {
        self.phase.insert(var, positive);
        // Restore from stash back to queue with a fresh nonce
        if let Some(mut priority) = self.stashed.remove(&var) {
            priority.nonce = self.rng.next_u64();
            self.queue.push(var, priority);
        }
    }

    /// Restore all stashed variables back to the queue.
    /// Call this when backtracking to Level::TOP to handle external input changes.
    pub(crate) fn restore_all_stashed(&mut self) {
        for (var, mut priority) in self.stashed.drain() {
            priority.nonce = self.rng.next_u64();
            self.queue.push(var, priority);
        }
    }

    pub(crate) fn get_phase(&self, var: Var) -> bool {
        self.phase.get(&var).copied().unwrap_or(false)
    }

    pub(crate) fn pick(&mut self, is_assigned: impl Fn(Var) -> bool) -> Option<Var> {
        loop {
            let (&var, _) = self.queue.peek()?;
            if !is_assigned(var) {
                return Some(var);
            }
            // Remove assigned variable and stash it (will be restored via set_phase)
            let (var, priority) = self.queue.pop().unwrap();
            self.stashed.insert(var, priority);
        }
    }
}
