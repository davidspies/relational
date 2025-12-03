//! VSIDS (Variable State Independent Decaying Sum) decision heuristic.
//!
//! Simple implementation: linear scan to find max activity.

use super::types::Var;

pub struct Vsids {
    /// Activity score per variable (indexed by var.raw() - 1)
    activity: Vec<f64>,
    /// Bump amount (increases for decay effect)
    bump: f64,
    /// Saved phase per variable
    phase: Vec<bool>,
}

impl Vsids {
    pub fn new(num_vars: u32) -> Self {
        Self {
            activity: vec![0.0; num_vars as usize],
            bump: 1.0,
            phase: vec![false; num_vars as usize],
        }
    }

    pub fn ensure_capacity(&mut self, num_vars: u32) {
        let n = num_vars as usize;
        if self.activity.len() < n {
            self.activity.resize(n, 0.0);
            self.phase.resize(n, false);
        }
    }

    pub fn bump(&mut self, var: Var) {
        let idx = var.raw() as usize - 1;
        if idx < self.activity.len() {
            self.activity[idx] += self.bump;
        }
    }

    pub fn decay(&mut self) {
        self.bump *= 1.05;
    }

    pub fn set_phase(&mut self, var: Var, positive: bool) {
        let idx = var.raw() as usize - 1;
        if idx < self.phase.len() {
            self.phase[idx] = positive;
        }
    }

    pub fn get_phase(&self, var: Var) -> bool {
        let idx = var.raw() as usize - 1;
        self.phase.get(idx).copied().unwrap_or(false)
    }

    pub fn pick(&self, is_assigned: impl Fn(Var) -> bool) -> Option<Var> {
        let mut best: Option<(Var, f64)> = None;
        for (idx, &act) in self.activity.iter().enumerate() {
            let var = Var::new((idx + 1) as u32);
            if is_assigned(var) {
                continue;
            }
            match best {
                None => best = Some((var, act)),
                Some((_, best_act)) if act > best_act => best = Some((var, act)),
                _ => {}
            }
        }
        best.map(|(v, _)| v)
    }
}
