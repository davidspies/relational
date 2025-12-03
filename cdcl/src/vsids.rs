//! VSIDS (Variable State Independent Decaying Sum) decision heuristic.
//!
//! Uses a priority queue for O(log n) pick and O(log n) bump.

use std::cmp::Ordering;
use std::collections::HashMap;

use priority_queue::PriorityQueue;

use super::types::Var;

/// Wrapper for f64 that implements Ord (for use as priority).
/// Higher values have higher priority.
#[derive(Clone, Copy, PartialEq)]
struct Activity(f64);

impl Eq for Activity {}

impl PartialOrd for Activity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Activity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}

pub struct Vsids {
    /// Priority queue: var -> activity (only unassigned variables)
    queue: PriorityQueue<Var, Activity>,
    /// Stashed activities for assigned variables (removed from queue)
    stashed: HashMap<Var, Activity>,
    /// Bump amount (increases for decay effect)
    bump: f64,
    /// Saved phase per variable
    phase: HashMap<Var, bool>,
}

impl Vsids {
    pub fn new(num_vars: u32) -> Self {
        let mut queue = PriorityQueue::with_capacity(num_vars as usize);
        // Initialize all variables with 0 activity
        for i in 1..=num_vars {
            queue.push(Var::new(i), Activity(0.0));
        }
        Self {
            queue,
            stashed: HashMap::new(),
            bump: 1.0,
            phase: HashMap::with_capacity(num_vars as usize),
        }
    }

    pub fn bump(&mut self, var: Var) {
        let bump = self.bump;
        // Try queue first, fall back to stashed
        if self.queue.get(&var).is_some() {
            self.queue.change_priority_by(&var, |p| p.0 += bump);
        } else if let Some(activity) = self.stashed.get_mut(&var) {
            activity.0 += bump;
        }
    }

    pub fn decay(&mut self) {
        self.bump *= 1.05;
    }

    pub fn set_phase(&mut self, var: Var, positive: bool) {
        self.phase.insert(var, positive);
        // Restore from stash back to queue
        if let Some(activity) = self.stashed.remove(&var) {
            self.queue.push(var, activity);
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
            let (var, activity) = self.queue.pop().unwrap();
            self.stashed.insert(var, activity);
        }
    }
}
