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

#[cfg(debug_assertions)]
use std::ffi::CString;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
#[cfg(debug_assertions)]
use std::process::Command;

#[cfg(debug_assertions)]
use roundingsat_sys::rs_set_proof_log;
use roundingsat_sys::{
    RsResult, RsSolver, RsViolatedConstraint, rs_add_clause, rs_add_pb_constraint,
    rs_clear_externals, rs_flush_proof_log, rs_free, rs_get_num_vars, rs_get_value, rs_new,
    rs_set_externals, rs_set_num_vars, rs_set_solution_callback, rs_solve,
};

/// Result of a solve operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolveResult {
    /// A satisfying assignment was found.
    Sat,
    /// The problem is unsatisfiable.
    Unsat,
    /// The problem is unsatisfiable under the current externals.
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

/// A term in a pseudo-boolean constraint: coefficient * literal.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Fields only read in debug builds
struct Term {
    lit: i32,
    coef: i32,
}

/// A stored constraint for sanity checking.
/// Represents: sum(term.coef * term.lit) >= rhs
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields only read in debug builds
struct StoredConstraint {
    terms: Vec<Term>,
    rhs: i64,
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
    /// All constraints added to the solver (for sanity checking)
    constraints: Vec<StoredConstraint>,
    /// Current externals (for sanity checking)
    externals: Vec<i32>,
    /// Path for proof logging (if enabled)
    proof_path: Option<PathBuf>,
    /// Writer for the OPB problem file
    opb_writer: Option<BufWriter<File>>,
    /// Number of constraints written to OPB file (for header update)
    #[allow(dead_code)] // Only used in debug builds
    opb_constraint_count: usize,
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
                constraints: Vec::new(),
                externals: Vec::new(),
                proof_path: None,
                opb_writer: None,
                opb_constraint_count: 0,
            })
        }
    }

    /// Enables proof logging to a file (debug builds only).
    ///
    /// Creates both `{path}.proof` (VeriPB proof) and `{path}.opb` (problem file).
    /// On UNSAT, automatically validates the proof using veripb.
    ///
    /// In release builds, this is a no-op for performance.
    ///
    /// # Panics
    ///
    /// Panics if the path contains a null byte or file creation fails.
    #[cfg(debug_assertions)]
    pub fn set_proof_log(&mut self, path: &Path) {
        let path_str = path.to_str().expect("Path must be valid UTF-8");
        let c_path = CString::new(path_str).expect("Path must not contain null bytes");
        unsafe { rs_set_proof_log(self.ptr, c_path.as_ptr()) }

        // Store path for later use
        self.proof_path = Some(path.to_path_buf());

        // Create OPB file
        let opb_path = path.with_extension("opb");
        let file = File::create(&opb_path).expect("Failed to create OPB file");
        let mut writer = BufWriter::new(file);

        // Write placeholder header (we'll need num_vars later)
        // VeriPB is lenient about headers, so we can write constraints without one
        // and add a comment instead
        writeln!(writer, "* OPB file generated by roundingsat").unwrap();

        // Write any already-added constraints
        for constraint in &self.constraints {
            Self::write_constraint_to_opb(&mut writer, constraint);
            self.opb_constraint_count += 1;
        }

        self.opb_writer = Some(writer);
    }

    /// Enables proof logging to a file (debug builds only).
    ///
    /// In release builds, this is a no-op for performance.
    #[cfg(not(debug_assertions))]
    pub fn set_proof_log(&mut self, _path: &Path) {
        // No-op in release builds
    }

    /// Writes a constraint in OPB format: +1 x1 +1 ~x2 >= 3 ;
    #[cfg(debug_assertions)]
    fn write_constraint_to_opb(writer: &mut BufWriter<File>, constraint: &StoredConstraint) {
        for term in &constraint.terms {
            let var = term.lit.abs();
            // Always include sign before coefficient
            if term.coef >= 0 {
                write!(writer, "+").unwrap();
            }
            if term.lit > 0 {
                write!(writer, "{} x{} ", term.coef, var).unwrap();
            } else {
                write!(writer, "{} ~x{} ", term.coef, var).unwrap();
            }
        }
        writeln!(writer, ">= {} ;", constraint.rhs).unwrap();
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

        let terms: Vec<Term> = lits
            .iter()
            .zip(coefs.iter())
            .map(|(&lit, &coef)| Term { lit, coef })
            .collect();
        let constraint = StoredConstraint { terms, rhs };

        // Write to OPB file BEFORE calling C++ - even if the add causes UNSAT,
        // the C++ side will have already logged `l N` to the proof referencing
        // this constraint, so it must be in the OPB file.
        #[cfg(debug_assertions)]
        if let Some(ref mut writer) = self.opb_writer {
            Self::write_constraint_to_opb(writer, &constraint);
            self.opb_constraint_count += 1;
        }

        let result = unsafe {
            rs_add_pb_constraint(self.ptr, lits.len(), lits.as_ptr(), coefs.as_ptr(), rhs)
        };
        if result < 0 {
            Err(SolverError::UnsatAtRoot)
        } else {
            self.constraints.push(constraint);
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

        let terms: Vec<Term> = lits.iter().map(|&lit| Term { lit, coef: 1 }).collect();
        let constraint = StoredConstraint { terms, rhs: 1 };

        // Write to OPB file BEFORE calling C++ - even if the add causes UNSAT,
        // the C++ side will have already logged `l N` to the proof referencing
        // this constraint, so it must be in the OPB file.
        #[cfg(debug_assertions)]
        if let Some(ref mut writer) = self.opb_writer {
            Self::write_constraint_to_opb(writer, &constraint);
            self.opb_constraint_count += 1;
        }

        let result = unsafe { rs_add_clause(self.ptr, lits.len(), lits.as_ptr()) };
        if result < 0 {
            Err(SolverError::UnsatAtRoot)
        } else {
            self.constraints.push(constraint);
            Ok(())
        }
    }

    /// Sets externals for the next solve call.
    ///
    /// externals are temporary assignments that can be cleared after solving.
    /// If the problem is unsatisfiable under the externals, `solve()` returns
    /// `SolveResult::Unsat`.
    ///
    /// # Errors
    ///
    /// Returns `SolverError::InvalidLiteral` if any literal references a variable outside 1..num_vars.
    pub fn set_externals(&mut self, assumps: &[i32]) -> Result<(), SolverError> {
        self.validate_literals(assumps)?;
        unsafe { rs_set_externals(self.ptr, assumps.len(), assumps.as_ptr()) }
        self.externals = assumps.to_vec();
        Ok(())
    }

    /// Clears all externals.
    pub fn clear_externals(&mut self) {
        unsafe { rs_clear_externals(self.ptr) }
        self.externals.clear();
    }

    /// Solves the current problem.
    ///
    /// Returns the result of the solve operation.
    ///
    /// # Panics
    ///
    /// Panics if the solution callback returns a constraint that is not actually
    /// violated under the current assignment. This indicates a bug in the callback.
    /// Also panics if proof logging is enabled and veripb validation fails on UNSAT.
    pub fn solve(&mut self) -> SolveResult {
        // Flush OPB writer before solving
        if let Some(ref mut writer) = self.opb_writer {
            writer.flush().unwrap();
        }

        let result = unsafe { rs_solve(self.ptr) };

        // Flush proof log after solve completes
        if self.proof_path.is_some() {
            unsafe { rs_flush_proof_log(self.ptr) };
        }

        if result == RsResult::CallbackError {
            panic!(
                "Solution callback returned a constraint that is not violated \
                under the current assignment. The constraint must have negative \
                slack (sum of satisfied terms < rhs) to be valid."
            );
        }
        let solve_result: SolveResult = result.into();

        #[cfg(debug_assertions)]
        match solve_result {
            SolveResult::Sat => {
                self.sanity_check_solution();
            }
            SolveResult::Unsat => {
                self.verify_proof_if_enabled();
            }
            _ => {}
        }

        solve_result
    }

    /// Verifies the proof using veripb if proof logging is enabled.
    #[cfg(debug_assertions)]
    fn verify_proof_if_enabled(&mut self) {
        let Some(ref proof_path) = self.proof_path else {
            return;
        };

        // Flush the OPB writer
        if let Some(ref mut writer) = self.opb_writer {
            writer.flush().unwrap();
        }

        // Flush the proof log to disk
        unsafe { rs_flush_proof_log(self.ptr) };

        let opb_path = proof_path.with_extension("opb");
        let proof_file = proof_path.with_extension("proof");

        // Try to find veripb in the venv
        let veripb_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".venv/bin/veripb");

        if !veripb_path.exists() {
            // veripb not installed, skip validation
            return;
        }

        // Copy OPB to temp file and append externals as singleton constraints
        let temp_opb = proof_path.with_extension("opb.tmp");
        {
            std::fs::copy(&opb_path, &temp_opb).expect("Failed to copy OPB file");

            // Append externals to OPB as singleton constraints
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&temp_opb)
                .expect("Failed to open temp OPB for appending");

            for &lit in &self.externals {
                let var = lit.abs();
                if lit > 0 {
                    writeln!(file, "+1 x{} >= 1 ;", var).unwrap();
                } else {
                    writeln!(file, "+1 ~x{} >= 1 ;", var).unwrap();
                }
            }
        }

        // Create temp proof: modify conclusion to derive contradiction from externals
        let temp_proof = proof_path.with_extension("proof.tmp");
        {
            let proof_content = std::fs::read_to_string(&proof_file).unwrap();

            // OPB constraint IDs for externals (appended after original constraints)
            let ext_opb_base = self.constraints.len() + 1;

            // Find the highest proof ID used, and the conclusion line
            let mut max_proof_id = 0u64;

            for line in proof_content.lines() {
                let trimmed = line.trim();
                // Lines like "e N ..." define constraint N
                if trimmed.starts_with('e') {
                    if let Some(id_str) =
                        trimmed.strip_prefix('e').unwrap().split_whitespace().next()
                    {
                        if let Ok(id) = id_str.parse::<u64>() {
                            max_proof_id = max_proof_id.max(id);
                        }
                    }
                }
                // Conclusion line: "c N 0"
                if trimmed.starts_with('c') && trimmed.ends_with(" 0") {
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if parts.len() == 3 {
                        let _conclusion_constraint_id = parts[1].parse::<u64>().ok();
                    }
                }
            }

            if self.externals.is_empty() {
                // No externals - verify with original files
                let output = Command::new(&veripb_path)
                    .arg("--requireUnsat")
                    .arg(&opb_path)
                    .arg(&proof_file)
                    .output()
                    .expect("Failed to run veripb");

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    // Copy files to a stable location for debugging
                    let debug_dir = PathBuf::from("/tmp/veripb_debug");
                    let _ = std::fs::create_dir_all(&debug_dir);
                    let _ = std::fs::copy(&opb_path, debug_dir.join("check_proof.opb"));
                    let _ = std::fs::copy(&proof_file, debug_dir.join("check_proof.proof"));
                    panic!(
                        "veripb verification failed!\n\
                         Debug files saved to: {}\n\
                         stdout: {}\n\
                         stderr: {}",
                        debug_dir.display(),
                        stdout,
                        stderr,
                    );
                }
                return;
            }

            // Build load commands for externals
            let mut load_commands = String::new();
            for i in 0..self.externals.len() {
                let opb_id = ext_opb_base + i;
                load_commands.push_str(&format!("l {}\n", opb_id));
            }

            // After loading externals, derive contradiction via RUP and conclude
            let rup_and_conclude = format!(
                "{}u >= 1 ;\nc {} 0\n",
                load_commands,
                max_proof_id + self.externals.len() as u64 + 1
            );

            // Check if there's an existing conclusion line to replace
            let has_conclusion = proof_content
                .lines()
                .any(|l| l.trim().starts_with('c') && l.trim().ends_with(" 0"));

            let modified: String = if has_conclusion {
                // Replace the conclusion line with: load externals + RUP + conclude
                proof_content
                    .lines()
                    .map(|l| {
                        let trimmed = l.trim();
                        if trimmed.starts_with('c') && trimmed.ends_with(" 0") {
                            rup_and_conclude.clone()
                        } else {
                            format!("{}\n", l)
                        }
                    })
                    .collect()
            } else {
                // No conclusion line - append our RUP + conclude at the end
                format!("{}{}", proof_content, rup_and_conclude)
            };

            std::fs::write(&temp_proof, &modified).expect("Failed to write temp proof");
        }

        let output = Command::new(&veripb_path)
            .arg("--requireUnsat")
            .arg(&temp_opb)
            .arg(&temp_proof)
            .output()
            .expect("Failed to run veripb");

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Copy files to a stable location for debugging
            let debug_dir = PathBuf::from("/tmp/veripb_debug");
            let _ = std::fs::create_dir_all(&debug_dir);
            let debug_opb = debug_dir.join("check_proof.opb");
            let debug_proof = debug_dir.join("check_proof.proof");
            let _ = std::fs::copy(&temp_opb, &debug_opb);
            let _ = std::fs::copy(&temp_proof, &debug_proof);
            // Clean up temp files
            let _ = std::fs::remove_file(&temp_opb);
            let _ = std::fs::remove_file(&temp_proof);
            panic!(
                "veripb verification failed!\n\
                 Debug files saved to: {}\n\
                 stdout: {}\n\
                 stderr: {}",
                debug_dir.display(),
                stdout,
                stderr,
            );
        }

        // Clean up temp files on success
        let _ = std::fs::remove_file(&temp_opb);
        let _ = std::fs::remove_file(&temp_proof);
    }

    /// Verifies that the current solution satisfies all constraints and externals.
    ///
    /// # Panics
    ///
    /// Panics if any constraint is violated or any external disagrees with the solution.
    #[cfg(debug_assertions)]
    fn sanity_check_solution(&self) {
        // Check all externals are satisfied
        for &lit in &self.externals {
            let var = lit.abs();
            let expected = lit > 0;
            let actual = self
                .get_value(var)
                .expect("Variable should have a value in SAT solution");
            assert!(
                actual == expected,
                "External {} violated: expected var {} = {}, got {}",
                lit,
                var,
                expected,
                actual
            );
        }

        // Check all constraints are satisfied
        for (i, constraint) in self.constraints.iter().enumerate() {
            let sum: i64 = constraint
                .terms
                .iter()
                .map(|term| {
                    let var = term.lit.abs();
                    let lit_true = term.lit > 0;
                    let var_true = self
                        .get_value(var)
                        .expect("Variable should have a value in SAT solution");
                    let lit_satisfied = lit_true == var_true;
                    if lit_satisfied { term.coef as i64 } else { 0 }
                })
                .sum();
            assert!(
                sum >= constraint.rhs,
                "Constraint {} violated: sum = {}, rhs = {}",
                i,
                sum,
                constraint.rhs
            );
        }
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
            // Write to OPB file if proof logging is enabled
            #[cfg(debug_assertions)]
            if let Some(ref mut writer) = solver.opb_writer {
                let constraint = StoredConstraint {
                    terms: violated
                        .lits
                        .iter()
                        .zip(violated.coefs.iter())
                        .map(|(&lit, &coef)| Term { lit, coef })
                        .collect(),
                    rhs: violated.rhs,
                };
                Solver::write_constraint_to_opb(writer, &constraint);
                solver.opb_constraint_count += 1;
            }

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
    fn test_externals() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(2);

        // x1 OR x2
        solver.add_clause(&[1, 2]).unwrap();

        // Assume NOT x1 AND NOT x2 -> should be unsat
        solver.set_externals(&[-1, -2]).unwrap();
        assert_eq!(solver.solve(), SolveResult::Unsat);

        // Clear externals -> should be SAT again
        solver.clear_externals();
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
    fn test_error_invalid_literal_externals() {
        let mut solver = Solver::new().unwrap();
        solver.set_num_vars(2);

        // Variable 5 doesn't exist
        let result = solver.set_externals(&[1, 5]);
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

    #[test]
    fn test_proof_logging_unsat() {
        let proof_dir = tempfile::tempdir().unwrap();
        let proof_base = proof_dir.path().join("unsat_test");

        let mut solver = Solver::new().unwrap();
        solver.set_proof_log(&proof_base);

        // Pigeonhole: 3 pigeons, 2 holes
        // Variables: 1=p1h1, 2=p1h2, 3=p2h1, 4=p2h2, 5=p3h1, 6=p3h2
        solver.set_num_vars(6);

        // Each pigeon in at least one hole
        solver.add_clause(&[1, 2]).unwrap();
        solver.add_clause(&[3, 4]).unwrap();
        solver.add_clause(&[5, 6]).unwrap();

        // At most one pigeon per hole
        solver.add_clause(&[-1, -3]).unwrap();
        solver.add_clause(&[-1, -5]).unwrap();
        solver.add_clause(&[-3, -5]).unwrap();
        solver.add_clause(&[-2, -4]).unwrap();
        solver.add_clause(&[-2, -6]).unwrap();
        solver.add_clause(&[-4, -6]).unwrap();

        let result = solver.solve();
        assert_eq!(result, SolveResult::Unsat);

        // Verify OPB file was created
        let opb_path = proof_base.with_extension("opb");
        assert!(opb_path.exists());
    }

    #[test]
    fn test_proof_logging_unsat_with_externals() {
        let proof_dir = tempfile::tempdir().unwrap();
        let proof_base = proof_dir.path().join("unsat_externals_test");

        let mut solver = Solver::new().unwrap();
        solver.set_proof_log(&proof_base);
        solver.set_num_vars(2);

        // x1 OR x2
        solver.add_clause(&[1, 2]).unwrap();

        // Assume NOT x1 AND NOT x2 -> should be UNSAT under these externals
        solver.set_externals(&[-1, -2]).unwrap();

        let result = solver.solve();
        assert_eq!(result, SolveResult::Unsat);

        // Verify OPB file exists
        let opb_path = proof_base.with_extension("opb");
        assert!(opb_path.exists());
    }

    #[test]
    fn debug_print_proof_with_externals() {
        use std::process::Command;

        let proof_dir = tempfile::tempdir().unwrap();
        let proof_base = proof_dir.path().join("test");

        let mut solver = Solver::new().unwrap();
        solver.set_proof_log(&proof_base);
        solver.set_num_vars(2);

        // x1 OR x2
        solver.add_clause(&[1, 2]).unwrap();

        // Assume NOT x1 AND NOT x2 -> should be UNSAT
        solver.set_externals(&[-1, -2]).unwrap();

        let result = solver.solve();
        eprintln!("Result: {:?}", result);

        // Print files
        let opb = std::fs::read_to_string(proof_base.with_extension("opb")).unwrap();
        let proof = std::fs::read_to_string(proof_base.with_extension("proof")).unwrap();

        eprintln!("=== OPB ===\n{}", opb);
        eprintln!("=== PROOF ===\n{}", proof);

        // Test manual verification with externals appended
        let temp_opb = proof_dir.path().join("temp.opb");
        let temp_proof = proof_dir.path().join("temp.proof");

        // Append externals to OPB
        {
            std::fs::copy(proof_base.with_extension("opb"), &temp_opb).unwrap();
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&temp_opb)
                .unwrap();
            use std::io::Write;
            // External -1 means x1 = false, so ~x1 >= 1
            writeln!(file, "+1 ~x1 >= 1 ;").unwrap();
            // External -2 means x2 = false, so ~x2 >= 1
            writeln!(file, "+1 ~x2 >= 1 ;").unwrap();
        }

        // Modify proof exactly as verify_proof_if_enabled does
        {
            // Calculate max_proof_id from original proof
            let mut max_proof_id = 0u64;
            for line in proof.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('e') {
                    if let Some(id_str) =
                        trimmed.strip_prefix('e').unwrap().split_whitespace().next()
                    {
                        if let Ok(id) = id_str.parse::<u64>() {
                            max_proof_id = max_proof_id.max(id);
                        }
                    }
                }
            }
            eprintln!("max_proof_id: {}", max_proof_id);

            // ext_opb_base = constraints.len() + 1 = 1 + 1 = 2
            let ext_opb_base = 2;
            let externals = vec![-1i32, -2i32];

            let mut load_commands = String::new();
            for i in 0..externals.len() {
                let opb_id = ext_opb_base + i;
                load_commands.push_str(&format!("l {}\n", opb_id));
            }

            let rup_and_conclude = format!(
                "{}u >= 1 ;\nc {} 0\n",
                load_commands,
                max_proof_id + externals.len() as u64 + 1
            );
            eprintln!("rup_and_conclude:\n{}", rup_and_conclude);

            // Replace the conclusion line
            let modified: String = proof
                .lines()
                .map(|l| {
                    let trimmed = l.trim();
                    if trimmed.starts_with('c') && trimmed.ends_with(" 0") {
                        rup_and_conclude.clone()
                    } else {
                        format!("{}\n", l)
                    }
                })
                .collect();

            std::fs::write(&temp_proof, &modified).unwrap();
        }

        eprintln!("=== TEMP OPB ===");
        eprintln!("{}", std::fs::read_to_string(&temp_opb).unwrap());
        eprintln!("=== TEMP PROOF ===");
        eprintln!("{}", std::fs::read_to_string(&temp_proof).unwrap());

        // Try veripb with verbose trace
        let veripb_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".venv/bin/veripb");

        if veripb_path.exists() {
            let output = Command::new(&veripb_path)
                .arg("--requireUnsat")
                .arg("--trace")
                .arg("-v")
                .arg(&temp_opb)
                .arg(&temp_proof)
                .output()
                .expect("Failed to run veripb");

            eprintln!("=== VeriPB Output ===");
            eprintln!("status: {:?}", output.status);
            eprintln!("stdout:\n{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("stderr:\n{}", String::from_utf8_lossy(&output.stderr));
        } else {
            eprintln!("veripb not found at {:?}", veripb_path);
        }
    }
}
