//! VSIDS (Variable State Independent Decaying Sum) decision heuristic.
//!
//! Uses a priority queue for O(log n) pick and O(log n) bump.

use std::cmp::Ordering;
use std::collections::HashMap;

use priority_queue::PriorityQueue;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

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

pub struct Vsids {
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
    pub fn new(num_vars: u32) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(0x5a7d3e1f9c2b8a04);
        let mut queue = PriorityQueue::with_capacity(num_vars as usize);
        // Initialize all variables with 0 activity and random nonces
        for i in 1..=num_vars {
            queue.push(Var::new(i), Priority { activity: 0.0, nonce: rng.random() });
        }
        Self {
            queue,
            stashed: HashMap::new(),
            bump: 1.0,
            phase: HashMap::with_capacity(num_vars as usize),
            rng,
        }
    }

    pub fn bump(&mut self, var: Var) {
        let bump = self.bump;
        // Try queue first, fall back to stashed
        if self.queue.get(&var).is_some() {
            self.queue.change_priority_by(&var, |p| p.activity += bump);
        } else if let Some(priority) = self.stashed.get_mut(&var) {
            priority.activity += bump;
        }
    }

    pub fn decay(&mut self) {
        self.bump *= 1.05;
    }

    pub fn set_phase(&mut self, var: Var, positive: bool) {
        self.phase.insert(var, positive);
        // Restore from stash back to queue with a fresh nonce
        if let Some(mut priority) = self.stashed.remove(&var) {
            priority.nonce = self.rng.random();
            self.queue.push(var, priority);
        }
    }

    pub fn get_phase(&self, var: Var) -> bool {
        self.phase.get(&var).copied().unwrap_or(false)
    }

    pub fn pick(&mut self, is_assigned: impl Fn(Var) -> bool) -> Option<Var> {
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
