//! Raw FFI bindings to RoundingSat PB solver.

use std::os::raw::c_int;

/// Opaque solver handle.
#[repr(C)]
pub struct RsSolver {
    _private: [u8; 0],
}

/// Solve result.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsResult {
    Sat = 0,
    Unsat = 1,
    Inconsistent = 2,
    Unknown = 3,
    CallbackError = 4,
}

/// Violated constraint returned by solution callback.
/// Represents: sum(coefs[i] * lits[i]) >= rhs
#[repr(C)]
pub struct RsViolatedConstraint {
    pub n: usize,
    pub lits: *const i32,
    pub coefs: *const i32,
    pub rhs: i64,
}

/// Solution callback function type.
/// Called when a solution is found during solving.
/// Returns NULL to accept the solution, or pointer to violated constraint.
pub type RsSolutionCallback = Option<
    unsafe extern "C" fn(
        solver: *mut RsSolver,
        user_data: *mut std::ffi::c_void,
    ) -> *mut RsViolatedConstraint,
>;

unsafe extern "C" {
    /// Create a new solver instance.
    pub fn rs_new() -> *mut RsSolver;

    /// Destroy a solver instance.
    pub fn rs_free(solver: *mut RsSolver);

    /// Set the number of variables (1..n).
    pub fn rs_set_num_vars(solver: *mut RsSolver, n: i32);

    /// Add a PB constraint: sum of (coefs[i] * lits[i]) >= rhs
    /// Returns 0 on success, -1 if UNSAT at root.
    pub fn rs_add_pb_constraint(
        solver: *mut RsSolver,
        n: usize,
        lits: *const i32,
        coefs: *const i32,
        rhs: i64,
    ) -> c_int;

    /// Add a clause (all coefficients = 1, rhs = 1).
    /// Returns 0 on success, -1 if UNSAT at root.
    pub fn rs_add_clause(solver: *mut RsSolver, n: usize, lits: *const i32) -> c_int;

    /// Set externals for the next solve call.
    pub fn rs_set_externals(solver: *mut RsSolver, n: usize, assumps: *const i32);

    /// Clear all externals.
    pub fn rs_clear_externals(solver: *mut RsSolver);

    /// Solve the current problem.
    pub fn rs_solve(solver: *mut RsSolver) -> RsResult;

    /// Get the value of a variable (1..n) in the solution.
    /// Returns 1 if true, 0 if false, -1 if no solution.
    pub fn rs_get_value(solver: *mut RsSolver, var: i32) -> c_int;

    /// Get number of variables.
    pub fn rs_get_num_vars(solver: *mut RsSolver) -> i32;

    /// Set a callback to be invoked when a solution is found.
    /// The callback can return a violated constraint to reject the solution.
    pub fn rs_set_solution_callback(
        solver: *mut RsSolver,
        callback: RsSolutionCallback,
        user_data: *mut std::ffi::c_void,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_simple_create_destroy() {
        unsafe {
            let solver = rs_new();
            assert!(!solver.is_null());
            rs_free(solver);
        }
    }

    #[test]
    fn test_b_two_create_destroy() {
        unsafe {
            let solver1 = rs_new();
            let solver2 = rs_new();
            assert!(!solver1.is_null());
            assert!(!solver2.is_null());
            rs_free(solver1);
            rs_free(solver2);
        }
    }

    #[test]
    fn test_c_two_solvers_set_vars() {
        unsafe {
            let solver1 = rs_new();
            let solver2 = rs_new();
            rs_set_num_vars(solver1, 2);
            rs_set_num_vars(solver2, 4);
            rs_free(solver1);
            rs_free(solver2);
        }
    }

    #[test]
    fn test_d1_one_solver_add_clause() {
        unsafe {
            let solver1 = rs_new();
            rs_set_num_vars(solver1, 2);
            let lits1 = [1i32, 2];
            rs_add_clause(solver1, 2, lits1.as_ptr());
            rs_free(solver1);
        }
    }

    #[test]
    fn test_d2_two_solvers_add_clause_first_only() {
        unsafe {
            let solver1 = rs_new();
            let solver2 = rs_new();
            rs_set_num_vars(solver1, 2);
            rs_set_num_vars(solver2, 4);
            let lits1 = [1i32, 2];
            rs_add_clause(solver1, 2, lits1.as_ptr());
            // No clause added to solver2
            rs_free(solver1);
            rs_free(solver2);
        }
    }

    #[test]
    fn test_d3_two_solvers_add_clause_both() {
        unsafe {
            let solver1 = rs_new();
            let solver2 = rs_new();
            rs_set_num_vars(solver1, 2);
            rs_set_num_vars(solver2, 4);
            let lits1 = [1i32, 2];
            let lits2 = [3i32];
            rs_add_clause(solver1, 2, lits1.as_ptr());
            rs_add_clause(solver2, 1, lits2.as_ptr());
            rs_free(solver1);
            rs_free(solver2);
        }
    }

    #[test]
    fn test_e_two_solvers_solve() {
        unsafe {
            let solver1 = rs_new();
            let solver2 = rs_new();
            rs_set_num_vars(solver1, 2);
            rs_set_num_vars(solver2, 4);
            let lits1 = [1i32, 2];
            let lits2 = [3i32];
            rs_add_clause(solver1, 2, lits1.as_ptr());
            rs_add_clause(solver2, 1, lits2.as_ptr());
            let r1 = rs_solve(solver1);
            let r2 = rs_solve(solver2);
            assert_eq!(r1, RsResult::Sat);
            assert_eq!(r2, RsResult::Sat);
            rs_free(solver1);
            rs_free(solver2);
        }
    }

    #[test]
    fn test_z_two_solvers_simultaneously() {
        unsafe {
            // Create two solvers at once (like ASP candidate + check)
            let solver1 = rs_new();
            let solver2 = rs_new();
            assert!(!solver1.is_null());
            assert!(!solver2.is_null());

            // Set up solver1: x1 OR x2
            rs_set_num_vars(solver1, 2);
            let lits = [1i32, 2];
            rs_add_clause(solver1, 2, lits.as_ptr());

            // Set up solver2: x3 AND x4
            rs_set_num_vars(solver2, 4);
            let lit3 = [3i32];
            let lit4 = [4i32];
            rs_add_clause(solver2, 1, lit3.as_ptr());
            rs_add_clause(solver2, 1, lit4.as_ptr());

            // Solve both
            let r1 = rs_solve(solver1);
            let r2 = rs_solve(solver2);
            assert_eq!(r1, RsResult::Sat);
            assert_eq!(r2, RsResult::Sat);

            // Check solutions are independent
            let v1 = rs_get_value(solver1, 1);
            let v2 = rs_get_value(solver1, 2);
            assert!(v1 == 1 || v2 == 1);

            let v3 = rs_get_value(solver2, 3);
            let v4 = rs_get_value(solver2, 4);
            assert_eq!(v3, 1);
            assert_eq!(v4, 1);

            rs_free(solver1);
            rs_free(solver2);
        }
    }

    #[test]
    fn test_roundingsat_api() {
        unsafe {
            // Test 1: Simple SAT
            let solver = rs_new();
            assert!(!solver.is_null());

            rs_set_num_vars(solver, 3);

            // Add clause: x1 OR x2
            let lits = [1i32, 2];
            rs_add_clause(solver, 2, lits.as_ptr());

            let result = rs_solve(solver);
            assert_eq!(result, RsResult::Sat);

            // At least one should be true
            let v1 = rs_get_value(solver, 1);
            let v2 = rs_get_value(solver, 2);
            assert!(v1 == 1 || v2 == 1);

            // Test 2: Add PB constraint and resolve
            // At least 2 of {x1, x2, x3} must be true: x1 + x2 + x3 >= 2
            let lits = [1i32, 2, 3];
            let coefs = [1i32, 1, 1];
            rs_add_pb_constraint(solver, 3, lits.as_ptr(), coefs.as_ptr(), 2);

            let result = rs_solve(solver);
            assert_eq!(result, RsResult::Sat);

            let v1 = rs_get_value(solver, 1);
            let v2 = rs_get_value(solver, 2);
            let v3 = rs_get_value(solver, 3);
            assert!(v1 + v2 + v3 >= 2);

            // Test 3: Assumptions making it inconsistent
            // Assume NOT x1 AND NOT x2 AND NOT x3 -> should be inconsistent
            let assumps = [-1i32, -2, -3];
            rs_set_externals(solver, 3, assumps.as_ptr());

            let result = rs_solve(solver);
            assert_eq!(result, RsResult::Inconsistent);

            // Test 4: Clear externals and solve again -> should be SAT
            rs_clear_externals(solver);
            let result = rs_solve(solver);
            assert_eq!(result, RsResult::Sat);

            rs_free(solver);
        }
    }
}
