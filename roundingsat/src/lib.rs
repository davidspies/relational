//! Safe Rust wrapper for the RoundingSat pseudo-boolean solver.
//!
//! RoundingSat is a pseudo-boolean (PB) solver that supports constraints of the form:
//! `sum(coef[i] * lit[i]) >= rhs`
//!
//! # Example
//!
//! ```
//! use roundingsat::{Solver, SolveResult};
//!
//! let mut solver = Solver::new().unwrap();
//! solver.set_num_vars(3);
//!
//! // Add clause: x1 OR x2 (i.e., x1 + x2 >= 1)
//! solver.add_clause(&[1, 2]).unwrap();
//!
//! // Add PB constraint: x1 + x2 + x3 >= 2
//! solver.add_pb_constraint(&[1, 2, 3], &[1, 1, 1], 2).unwrap();
//!
//! assert_eq!(solver.solve(), SolveResult::Sat);
//!
//! // At least 2 of the 3 variables must be true
//! let count = (1..=3).filter(|&v| solver.get_value(v) == Some(true)).count();
//! assert!(count >= 2);
//! ```

use std::marker::PhantomData;

use roundingsat_sys::{
    RsResult, RsSolver, RsViolatedConstraint, rs_add_clause, rs_add_pb_constraint,
    rs_clear_assumptions, rs_free, rs_get_num_vars, rs_get_value, rs_new, rs_set_assumptions,
    rs_set_num_vars, rs_set_solution_callback, rs_solve,
};

/// Result of a solve operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolveResult {
    /// A satisfying assignment was found.
    Sat,
    /// The problem is unsatisfiable.
    Unsat,
    /// The problem is unsatisfiable under the current assumptions.
    Inconsistent,
    /// The solver could not determine satisfiability.
    Unknown,
}

impl From<RsResult> for SolveResult {
    fn from(r: RsResult) -> Self {
        match r {
            RsResult::Sat => SolveResult::Sat,
            RsResult::Unsat => SolveResult::Unsat,
            RsResult::Inconsistent => SolveResult::Inconsistent,
            RsResult::Unknown => SolveResult::Unknown,
            RsResult::CallbackError => {
                // This should already be caught by solve(), but panic here as a fallback
                panic!("Callback error: constraint returned was not violated")
            }
        }
    }
}

/// Error type for solver operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolverError {
    /// Failed to create a new solver instance.
    CreationFailed,
    /// Adding a constraint made the problem unsatisfiable at root level.
    UnsatAtRoot,
    /// Mismatched array lengths for PB constraint.
    MismatchedLengths { lits: usize, coefs: usize },
    /// A literal references a variable outside the valid range.
    InvalidLiteral { lit: i32, num_vars: i32 },
}

impl std::fmt::Display for SolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolverError::CreationFailed => write!(f, "failed to create solver instance"),
            SolverError::UnsatAtRoot => write!(f, "constraint made problem unsatisfiable at root"),
            SolverError::MismatchedLengths { lits, coefs } => {
                write!(
                    f,
                    "mismatched lengths: {} literals, {} coefficients",
                    lits, coefs
                )
            }
            SolverError::InvalidLiteral { lit, num_vars } => {
                write!(
                    f,
                    "literal {} references variable outside range 1..{} (use set_num_vars first)",
                    lit, num_vars
                )
            }
        }
    }
}

impl std::error::Error for SolverError {}

/// A violated constraint to be learned by the solver.
/// Represents: sum(coefs[i] * lits[i]) >= rhs
#[derive(Debug, Clone)]
pub struct ViolatedConstraint {
    pub lits: Vec<i32>,
    pub coefs: Vec<i32>,
    pub rhs: i64,
}

/// Read-only view of the current variable assignment.
/// Passed to the solution callback to query variable values.
pub struct Assignment<'a> {
    ptr: *mut RsSolver,
    _marker: PhantomData<&'a ()>,
}

impl Assignment<'_> {
    /// Get the value of a variable in the current assignment.
    pub fn get_value(&self, var: i32) -> Option<bool> {
        let result = unsafe { rs_get_value(self.ptr, var) };
        match result {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        }
    }

    /// Get the number of variables.
    pub fn num_vars(&self) -> i32 {
        unsafe { rs_get_num_vars(self.ptr) }
    }
}

/// Callback type for solution checking.
/// Returns `Some(constraint)` to reject the solution and continue searching,
/// or `None` to accept the solution.
type SolutionCallbackBox = Box<dyn FnMut(&Assignment) -> Option<ViolatedConstraint>>;

/// A pseudo-boolean constraint solver.
///
/// Variables are represented as positive integers (1, 2, 3, ...).
/// Literals are signed integers where positive means the variable is true,
/// and negative means the variable is false.
pub struct Solver {
    ptr: *mut RsSolver,
    /// Stored callback closure (must stay alive while solver uses it)
    callback: Option<Box<SolutionCallbackBox>>,
    /// Storage for the violated constraint returned by callback
    violated_storage: Option<ViolatedConstraint>,
    /// Storage for the FFI struct (avoids allocation each callback)
    violated_ffi: RsViolatedConstraint,
}

// Safety: RsSolver instances are independent and don't share mutable state
// through the API we expose. Each solver has its own internal state.
unsafe impl Send for Solver {}

impl Solver {
    /// Creates a new solver instance.
    pub fn new() -> Result<Self, SolverError> {
        let ptr = unsafe { rs_new() };
        if ptr.is_null() {
            Err(SolverError::CreationFailed)
        } else {
            Ok(Self {
                ptr,
                callback: None,
                violated_storage: None,
                violated_ffi: RsViolatedConstraint {
                    n: 0,
                    lits: std::ptr::null(),
                    coefs: std::ptr::null(),
                    rhs: 0,
                },
            })
        }
    }

    /// Sets the number of variables (1..n).
    ///
    /// Variables are numbered starting from 1.
    pub fn set_num_vars(&mut self, n: i32) {
        unsafe { rs_set_num_vars(self.ptr, n) }
    }

    /// Returns the current number of variables.
    pub fn num_vars(&self) -> i32 {
        unsafe { rs_get_num_vars(self.ptr) }
    }

    /// Validates that all literals reference variables in the valid range.
    fn validate_literals(&self, lits: &[i32]) -> Result<(), SolverError> {
        let n = self.num_vars();
        for &lit in lits {
            // lit must be non-zero and |lit| must be <= num_vars
            if lit == 0 || lit.abs() > n {
                return Err(SolverError::InvalidLiteral { lit, num_vars: n });
            }
        }
        Ok(())
    }

    /// Adds a pseudo-boolean constraint: `sum(coefs[i] * lits[i]) >= rhs`
    ///
    /// # Arguments
    ///
    /// * `lits` - Literals (positive for true, negative for false)
    /// * `coefs` - Coefficients for each literal
    /// * `rhs` - Right-hand side (the constraint must sum to at least this value)
    ///
    /// # Errors
    ///
    /// Returns `SolverError::MismatchedLengths` if `lits` and `coefs` have different lengths.
    /// Returns `SolverError::InvalidLiteral` if any literal references a variable outside 1..num_vars.
    /// Returns `SolverError::UnsatAtRoot` if this constraint makes the problem unsatisfiable.
    pub fn add_pb_constraint(
        &mut self,
        lits: &[i32],
        coefs: &[i32],
        rhs: i64,
    ) -> Result<(), SolverError> {
        if lits.len() != coefs.len() {
            return Err(SolverError::MismatchedLengths {
                lits: lits.len(),
                coefs: coefs.len(),
            });
        }
        self.validate_literals(lits)?;
        let result = unsafe {
            rs_add_pb_constraint(self.ptr, lits.len(), lits.as_ptr(), coefs.as_ptr(), rhs)
        };
        if result < 0 {
            Err(SolverError::UnsatAtRoot)
        } else {
            Ok(())
        }
    }

    /// Adds a clause (disjunction of literals).
    ///
    /// This is equivalent to `add_pb_constraint(lits, &[1, 1, ...], 1)`.
    ///
    /// # Errors
    ///
    /// Returns `SolverError::InvalidLiteral` if any literal references a variable outside 1..num_vars.
    /// Returns `SolverError::UnsatAtRoot` if this clause makes the problem unsatisfiable.
    pub fn add_clause(&mut self, lits: &[i32]) -> Result<(), SolverError> {
        self.validate_literals(lits)?;
        let result = unsafe { rs_add_clause(self.ptr, lits.len(), lits.as_ptr()) };
        if result < 0 {
            Err(SolverError::UnsatAtRoot)
        } else {
            Ok(())
        }
    }

    /// Sets assumptions for the next solve call.
    ///
    /// Assumptions are temporary assignments that can be cleared after solving.
    /// If the problem is unsatisfiable under the assumptions, `solve()` returns
    /// `SolveResult::Inconsistent`.
    ///
    /// # Errors
    ///
    /// Returns `SolverError::InvalidLiteral` if any literal references a variable outside 1..num_vars.
    pub fn set_assumptions(&mut self, assumps: &[i32]) -> Result<(), SolverError> {
        self.validate_literals(assumps)?;
        unsafe { rs_set_assumptions(self.ptr, assumps.len(), assumps.as_ptr()) }
        Ok(())
    }

    /// Clears all assumptions.
    pub fn clear_assumptions(&mut self) {
        unsafe { rs_clear_assumptions(self.ptr) }
    }

    /// Solves the current problem.
    ///
    /// Returns the result of the solve operation.
    ///
    /// # Panics
    ///
    /// Panics if the solution callback returns a constraint that is not actually
    /// violated under the current assignment. This indicates a bug in the callback.
    pub fn solve(&mut self) -> SolveResult {
        let result = unsafe { rs_solve(self.ptr) };
        if result == RsResult::CallbackError {
            panic!(
                "Solution callback returned a constraint that is not violated \
                under the current assignment. The constraint must have negative \
                slack (sum of satisfied terms < rhs) to be valid."
            );
        }
        result.into()
    }

    /// Gets the value of a variable in the solution.
    ///
    /// # Arguments
    ///
    /// * `var` - Variable number (must be positive)
    ///
    /// # Returns
    ///
    /// * `Some(true)` if the variable is true in the solution
    /// * `Some(false)` if the variable is false in the solution
    /// * `None` if no solution exists or the variable is invalid
    pub fn get_value(&self, var: i32) -> Option<bool> {
        let result = unsafe { rs_get_value(self.ptr, var) };
        match result {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        }
    }

    /// Gets the values of all variables in the solution.
    ///
    /// Returns a vector where index `i` contains the value of variable `i+1`.
    /// Returns `None` if no solution exists.
    pub fn get_model(&self) -> Option<Vec<bool>> {
        let n = self.num_vars();
        if n <= 0 {
            return Some(vec![]);
        }
        let mut model = Vec::with_capacity(n as usize);
        for var in 1..=n {
            match self.get_value(var) {
                Some(v) => model.push(v),
                None => return None,
            }
        }
        Some(model)
    }

    /// Sets a callback to be invoked when a solution is found during solving.
    ///
    /// The callback receives a read-only `Assignment` to query variable values.
    /// Return `Some(constraint)` to reject the solution and continue searching.
    /// Return `None` to accept the solution (solver returns SAT).
    ///
    /// The callback will be called during `solve()`. If it returns a violated
    /// constraint, the solver performs conflict analysis and backtracks instead
    /// of returning SAT.
    pub fn set_solution_callback<F>(&mut self, callback: F)
    where
        F: FnMut(&Assignment) -> Option<ViolatedConstraint> + 'static,
    {
        self.callback = Some(Box::new(Box::new(callback)));
        // Set up the C callback trampoline
        unsafe {
            rs_set_solution_callback(
                self.ptr,
                Some(solution_callback_trampoline),
                self as *mut Solver as *mut std::ffi::c_void,
            );
        }
    }

    /// Clears the solution callback.
    pub fn clear_solution_callback(&mut self) {
        unsafe {
            rs_set_solution_callback(self.ptr, None, std::ptr::null_mut());
        }
        self.callback = None;
        self.violated_storage = None;
    }
}

/// FFI trampoline for solution callback.
/// Called from C++ when a solution is found.
unsafe extern "C" fn solution_callback_trampoline(
    solver_ptr: *mut RsSolver,
    user_data: *mut std::ffi::c_void,
) -> *mut RsViolatedConstraint {
    // SAFETY: user_data is a valid pointer to a Solver set up by set_solution_callback
    let solver = unsafe { &mut *(user_data as *mut Solver) };

    // Create an Assignment view for the callback
    let assignment = Assignment {
        ptr: solver_ptr,
        _marker: PhantomData,
    };

    // Call the Rust callback
    let result = if let Some(callback) = &mut solver.callback {
        callback(&assignment)
    } else {
        None
    };

    match result {
        None => std::ptr::null_mut(),
        Some(violated) => {
            // Store the constraint in the solver so it outlives this function
            solver.violated_storage = Some(violated);
            let stored = solver.violated_storage.as_ref().unwrap();

            // Update the FFI struct to point to our stored data
            solver.violated_ffi.n = stored.lits.len();
            solver.violated_ffi.lits = stored.lits.as_ptr();
            solver.violated_ffi.coefs = stored.coefs.as_ptr();
            solver.violated_ffi.rhs = stored.rhs;

            // Return a pointer to our stored FFI struct
            &mut solver.violated_ffi as *mut RsViolatedConstraint
        }
    }
}

impl Drop for Solver {
    fn drop(&mut self) {
        unsafe { rs_free(self.ptr) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_sat() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(2);

        // x1 OR x2
        solver.add_clause(&[1, 2]).unwrap();

        assert_eq!(solver.solve(), SolveResult::Sat);

        // At least one should be true
        let v1 = solver.get_value(1).unwrap();
        let v2 = solver.get_value(2).unwrap();
        assert!(v1 || v2);
    }

    #[test]
    fn test_simple_unsat() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(1);

        // x1 AND NOT x1 - detected as unsat at root when adding second clause
        solver.add_clause(&[1]).unwrap();
        let result = solver.add_clause(&[-1]);
        assert!(matches!(result, Err(SolverError::UnsatAtRoot)));
    }

    #[test]
    fn test_pb_constraint() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(3);

        // At least 2 of {x1, x2, x3} must be true
        solver.add_pb_constraint(&[1, 2, 3], &[1, 1, 1], 2).unwrap();

        assert_eq!(solver.solve(), SolveResult::Sat);

        let count = (1..=3)
            .filter(|&v| solver.get_value(v) == Some(true))
            .count();
        assert!(count >= 2);
    }

    #[test]
    fn test_assumptions() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(2);

        // x1 OR x2
        solver.add_clause(&[1, 2]).unwrap();

        // Assume NOT x1 AND NOT x2 -> should be inconsistent
        solver.set_assumptions(&[-1, -2]).unwrap();
        assert_eq!(solver.solve(), SolveResult::Inconsistent);

        // Clear assumptions -> should be SAT again
        solver.clear_assumptions();
        assert_eq!(solver.solve(), SolveResult::Sat);
    }

    #[test]
    fn test_get_model() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(3);

        // Force all variables true
        solver.add_clause(&[1]).unwrap();
        solver.add_clause(&[2]).unwrap();
        solver.add_clause(&[3]).unwrap();

        assert_eq!(solver.solve(), SolveResult::Sat);

        let model = solver.get_model().unwrap();
        assert_eq!(model, vec![true, true, true]);
    }

    #[test]
    fn test_weighted_pb() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(3);

        // 2*x1 + 3*x2 + 1*x3 >= 4
        // This requires either x2 + something, or x1 + x2, etc.
        solver.add_pb_constraint(&[1, 2, 3], &[2, 3, 1], 4).unwrap();

        assert_eq!(solver.solve(), SolveResult::Sat);

        let v1 = solver.get_value(1).unwrap();
        let v2 = solver.get_value(2).unwrap();
        let v3 = solver.get_value(3).unwrap();

        let sum = (if v1 { 2 } else { 0 }) + (if v2 { 3 } else { 0 }) + (if v3 { 1 } else { 0 });
        assert!(sum >= 4);
    }

    #[test]
    fn test_two_solvers() {
        let mut solver1 = Solver::new().unwrap();
        let mut solver2 = Solver::new().unwrap();

        solver1.set_num_vars(2);
        solver2.set_num_vars(2);

        // solver1: x1 OR x2
        solver1.add_clause(&[1, 2]).unwrap();

        // solver2: x1 AND x2
        solver2.add_clause(&[1]).unwrap();
        solver2.add_clause(&[2]).unwrap();

        assert_eq!(solver1.solve(), SolveResult::Sat);
        assert_eq!(solver2.solve(), SolveResult::Sat);

        // solver2 must have both true
        assert_eq!(solver2.get_value(1), Some(true));
        assert_eq!(solver2.get_value(2), Some(true));
    }

    #[test]
    fn test_error_mismatched_lengths() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(3);

        let result = solver.add_pb_constraint(&[1, 2, 3], &[1, 1], 2);
        assert!(matches!(result, Err(SolverError::MismatchedLengths { .. })));
    }

    #[test]
    fn test_error_invalid_literal_out_of_range() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(2);

        // Variable 3 doesn't exist (only 1 and 2)
        let result = solver.add_clause(&[1, 3]);
        assert!(matches!(
            result,
            Err(SolverError::InvalidLiteral {
                lit: 3,
                num_vars: 2
            })
        ));

        // Negative out of range
        let result = solver.add_clause(&[-5]);
        assert!(matches!(
            result,
            Err(SolverError::InvalidLiteral {
                lit: -5,
                num_vars: 2
            })
        ));

        // Zero literal is invalid
        let result = solver.add_clause(&[0]);
        assert!(matches!(
            result,
            Err(SolverError::InvalidLiteral {
                lit: 0,
                num_vars: 2
            })
        ));
    }

    #[test]
    fn test_error_invalid_literal_pb() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(2);

        // Variable 10 doesn't exist
        let result = solver.add_pb_constraint(&[1, 10], &[1, 1], 1);
        assert!(matches!(
            result,
            Err(SolverError::InvalidLiteral {
                lit: 10,
                num_vars: 2
            })
        ));
    }

    #[test]
    fn test_error_invalid_literal_assumptions() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(2);

        // Variable 5 doesn't exist
        let result = solver.set_assumptions(&[1, 5]);
        assert!(matches!(
            result,
            Err(SolverError::InvalidLiteral {
                lit: 5,
                num_vars: 2
            })
        ));
    }

    #[test]
    fn test_sequential_solvers() {
        // First solver - SAT
        {
            let mut solver = Solver::new().unwrap();
            solver.set_num_vars(2);
            solver.add_clause(&[1, 2]).unwrap();
            assert_eq!(solver.solve(), SolveResult::Sat);
        }

        // Second solver - also SAT (tests that solver cleanup works correctly)
        {
            let mut solver = Solver::new().unwrap();
            solver.set_num_vars(2);
            solver.add_clause(&[1]).unwrap();
            solver.add_clause(&[2]).unwrap();
            assert_eq!(solver.solve(), SolveResult::Sat);
        }
    }

    #[test]
    fn test_solution_callback_rejects_solution() {
        // Problem: x1 AND x2 (both must be true)
        // Only solution: (1,1)
        // Callback will reject (1,1), so result should be UNSAT
        // Then we'll test with a problem that has two solutions where one is rejected

        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(2);

        // XOR: exactly one of x1, x2 must be true
        // Solutions: (1,0) and (0,1)
        solver.add_clause(&[1, 2]).unwrap(); // x1 OR x2
        solver.add_clause(&[-1, -2]).unwrap(); // NOT x1 OR NOT x2

        // Callback rejects (1,0) by adding constraint: -x1 OR x2 (i.e., x1 implies x2)
        solver.set_solution_callback(move |assignment: &Assignment| {
            let x1 = assignment.get_value(1).unwrap();
            let x2 = assignment.get_value(2).unwrap();

            if x1 && !x2 {
                // Reject (1,0) - return constraint: -x1 + x2 >= 1
                // This means if x1 is true, x2 must be true
                Some(ViolatedConstraint {
                    lits: vec![-1, 2],
                    coefs: vec![1, 1],
                    rhs: 1,
                })
            } else {
                // Accept this solution
                None
            }
        });

        let result = solver.solve();
        assert_eq!(result, SolveResult::Sat);

        // The only remaining solution is (0,1)
        let x1 = solver.get_value(1).unwrap();
        let x2 = solver.get_value(2).unwrap();
        assert!(!x1 && x2, "Expected (0,1) but got ({}, {})", x1, x2);
    }

    #[test]
    fn test_solution_callback_all_rejected_is_unsat() {
        // Problem: x1 (just one solution: x1=true)
        // Callback rejects x1=true, so no valid solutions exist

        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(1);
        solver.add_clause(&[1]).unwrap(); // x1 must be true

        solver.set_solution_callback(|assignment: &Assignment| {
            let x1 = assignment.get_value(1).unwrap();
            if x1 {
                // Reject x1=true by saying x1 must be false: -x1 >= 1
                Some(ViolatedConstraint {
                    lits: vec![-1],
                    coefs: vec![1],
                    rhs: 1,
                })
            } else {
                None
            }
        });

        let result = solver.solve();
        // Should be UNSAT because we require x1=true but callback rejects it
        assert_eq!(result, SolveResult::Unsat);
    }

    #[test]
    fn test_clear_solution_callback() {
        use std::cell::Cell;
        use std::rc::Rc;

        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(1);
        solver.add_clause(&[1]).unwrap();

        let call_count = Rc::new(Cell::new(0));
        let call_count_clone = call_count.clone();

        solver.set_solution_callback(move |_: &Assignment| {
            call_count_clone.set(call_count_clone.get() + 1);
            None // Accept all solutions
        });

        // First solve - callback should be called
        solver.solve();
        assert_eq!(call_count.get(), 1);

        // Clear callback
        solver.clear_solution_callback();

        // Second solve - callback should NOT be called
        solver.solve();
        assert_eq!(call_count.get(), 1); // Still 1, not 2
    }

    #[test]
    #[should_panic(expected = "not violated")]
    fn test_callback_error_on_invalid_constraint() {
        // If callback returns a constraint that's not actually violated,
        // the solver should panic.

        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(1);
        solver.add_clause(&[1]).unwrap(); // x1 must be true

        solver.set_solution_callback(|_: &Assignment| {
            // Return a constraint that's ALREADY SATISFIED: x1 >= 1
            // Since x1=true, this is satisfied, not violated!
            Some(ViolatedConstraint {
                lits: vec![1],
                coefs: vec![1],
                rhs: 1,
            })
        });

        // This should panic because the constraint is not violated
        solver.solve();
    }
}
