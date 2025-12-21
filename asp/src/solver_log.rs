//! Logging and replay infrastructure for debugging ASP solver operations.

use cdcl::{Lit, Var};
use serde::{Deserialize, Serialize};

/// A logged operation on the solver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SolverOp {
    /// Create candidate solver with given number of variables.
    CreateCandSolver { num_vars: u32 },
    /// Create check solver with given number of variables.
    CreateCheckSolver { num_vars: u32 },
    /// Add clause to candidate solver.
    CandAddClause { id: u32, lits: Vec<i32> },
    /// Add PB constraint to candidate solver.
    CandAddPB {
        id: u32,
        terms: Vec<(i32, i64)>,
        bound: i64,
    },
    /// Add clause to check solver.
    CheckAddClause { id: u32, lits: Vec<i32> },
    /// Add PB constraint to check solver.
    CheckAddPB {
        id: u32,
        terms: Vec<(i32, i64)>,
        bound: i64,
    },
    /// Solve candidate solver.
    CandSolve,
    /// Result of candidate solve.
    CandResult { sat: bool },
    /// Solve check solver.
    CheckSolve,
    /// Result of check solve.
    CheckResult { sat: bool },
    /// Backtrack candidate solver.
    CandBacktrack { level: u32 },
    /// Backtrack check solver.
    CheckBacktrack { level: u32 },
    /// Get candidate assignment.
    CandGetAssignment { assignment: Vec<(u32, bool)> },
    /// Get check assignment.
    CheckGetAssignment { assignment: Vec<(u32, bool)> },
}

/// Log of solver operations.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SolverLog {
    pub ops: Vec<SolverOp>,
}

impl SolverLog {
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    pub fn log(&mut self, op: SolverOp) {
        self.ops.push(op);
    }

    /// Convert Lit to i32 for serialization.
    pub fn lit_to_i32(lit: Lit) -> i32 {
        let var = lit.var().raw() as i32;
        if lit.is_positive() { var } else { -var }
    }

    /// Convert i32 to Lit for deserialization.
    pub fn i32_to_lit(i: i32) -> Lit {
        let var = Var::new(i.unsigned_abs());
        if i > 0 { Lit::pos(var) } else { Lit::neg(var) }
    }

    /// Write log to file.
    pub fn write_to_file(&self, path: &str) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Read log from file.
    pub fn read_from_file(path: &str) -> std::io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}
