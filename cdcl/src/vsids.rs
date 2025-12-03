//! VSIDS (Variable State Independent Decaying Sum) decision heuristic.
//!
//! Variables involved in recent conflicts have higher activity scores,
//! guiding the solver to focus on "hot" variables.

use std::collections::BinaryHeap;

use crate::types::Var;

/// Heap entry that compares by activity (highest first).
#[derive(Clone, Copy, PartialEq)]
struct HeapEntry {
    activity: f64,
    var: Var,
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.activity
            .partial_cmp(&other.activity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| other.var.raw().cmp(&self.var.raw()))
    }
}

/// VSIDS decision heuristic state.
pub struct Vsids {
    /// Activity score for each variable (indexed by var.raw() - 1).
    activity: Vec<f64>,
    /// Current activity increment (grows with decay to avoid rescaling).
    activity_inc: f64,
    /// Decay factor applied after each conflict.
    decay_factor: f64,
    /// Phase saving: last polarity assigned to each variable.
    phase: Vec<bool>,
    /// Max-heap for efficient picking.
    heap: BinaryHeap<HeapEntry>,
}

impl Vsids {
    /// Create a new VSIDS heuristic for the given number of variables.
    pub fn new(num_vars: u32) -> Self {
        Self {
            activity: vec![0.0; num_vars as usize],
            activity_inc: 1.0,
            decay_factor: 0.95,
            phase: vec![false; num_vars as usize],
            heap: BinaryHeap::new(),
        }
    }

    /// Bump activity for a variable (call for variables in conflict/learned clause).
    pub fn bump(&mut self, var: Var) {
        let idx = var.raw() as usize - 1;
        if idx < self.activity.len() {
            self.activity[idx] += self.activity_inc;
            self.heap.push(HeapEntry {
                activity: self.activity[idx],
                var,
            });
        }
    }

    /// Decay all activities (call after each conflict).
    pub fn decay(&mut self) {
        self.activity_inc /= self.decay_factor;
    }

    /// Pick the unassigned variable with highest activity.
    /// Returns (variable, num_rebuilds).
    pub fn pick(&mut self, is_assigned: impl Fn(Var) -> bool) -> (Option<Var>, u64) {
        // Try heap first - skip assigned variables
        while let Some(entry) = self.heap.pop() {
            if !is_assigned(entry.var) {
                return (Some(entry.var), 0);
            }
        }

        // Heap exhausted - rebuild with all variables and try again
        self.rebuild_heap();

        while let Some(entry) = self.heap.pop() {
            if !is_assigned(entry.var) {
                return (Some(entry.var), 1);
            }
        }
        (None, 1)
    }

    fn rebuild_heap(&mut self) {
        self.heap.clear();
        for (i, &act) in self.activity.iter().enumerate() {
            self.heap.push(HeapEntry {
                activity: act,
                var: Var::new((i + 1) as u32),
            });
        }
    }

    /// Get the saved phase (polarity) for a variable.
    pub fn get_phase(&self, var: Var) -> bool {
        let idx = var.raw() as usize - 1;
        if idx < self.phase.len() {
            self.phase[idx]
        } else {
            false
        }
    }

    /// Save the phase for a variable when it's assigned.
    pub fn set_phase(&mut self, var: Var, positive: bool) {
        let idx = var.raw() as usize - 1;
        if idx < self.phase.len() {
            self.phase[idx] = positive;
        }
    }

    /// Ensure capacity for at least `num_vars` variables.
    pub fn ensure_capacity(&mut self, num_vars: u32) {
        let needed = num_vars as usize;
        if self.activity.len() < needed {
            let old_len = self.activity.len();
            self.activity.resize(needed, 0.0);
            self.phase.resize(needed, false);
            // Add new variables to heap
            for i in (old_len + 1)..=needed {
                self.heap.push(HeapEntry {
                    activity: 0.0,
                    var: Var::new(i as u32),
                });
            }
        }
    }
}
