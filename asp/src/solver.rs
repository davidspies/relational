//! ASP solver using RoundingSat PB solver.
//!
//! Architecture:
//! - Candidate solver: finds candidate answer sets
//! - Check solver: checks if a strictly smaller model exists (unfounded set detection)
//! - Both solvers are created once; constraints are added incrementally to cand_solver,
//!   and check_solver uses externals for the candidate assignment.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use contiguous_data::HashSet;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use roundingsat::{Assignment, SolveResult, Solver, SolverError, ViolatedConstraint};

use crate::encoding::{Clause, EncodedProgram, PBConstraint, Var, VarLayout, neg, pos};
use crate::encoding::{encode_program, generate_loop_constraint};
use crate::types::{Atom, Program};

/// An answer set (stable model).
pub type AnswerSet = HashSet<Atom>;

/// Extract unfounded set: atoms in candidate but not in check.
/// Standalone version for use in callbacks.
fn extract_unfounded_set_standalone(
    layout: &VarLayout,
    cand_assignment: &HashMap<Var, bool>,
    check_solver: &Solver,
) -> Vec<Atom> {
    let mut unfounded = Vec::new();

    for atom_id in 2..=layout.num_atoms {
        let atom = Atom(atom_id);
        let cand_var = layout.cand(atom);
        let check_var = layout.check(atom);

        let in_cand = cand_assignment
            .get(&cand_var)
            .copied()
            .expect("cand atom should have value");
        let in_check = check_solver
            .get_value(check_var)
            .expect("check atom should have value");

        if in_cand && !in_check {
            unfounded.push(atom);
        }
    }

    unfounded
}

/// ASP Solver using RoundingSat PB solver.
pub struct AspSolver {
    program: Program,
    encoded: EncodedProgram,
    /// Fast lookup: atom → symbol name
    atom_names: HashMap<Atom, String>,
    /// Seeded RNG for deterministic behavior
    rng: StdRng,
}

impl AspSolver {
    /// Create a new ASP solver for the given program.
    pub fn new(program: Program) -> Self {
        let encoded = encode_program(&program);

        // Build atom→name index for fast lookups
        let atom_names: HashMap<Atom, String> = program
            .symbols
            .iter()
            .map(|(atom, name)| (*atom, name.clone()))
            .collect();

        AspSolver {
            program,
            encoded,
            atom_names,
            rng: StdRng::seed_from_u64(42),
        }
    }

    /// Find all stable models of the program.
    pub fn solve(&mut self) -> Vec<AnswerSet> {
        self.solve_n(0)
    }

    /// Find up to `limit` stable models (0 = unlimited).
    pub fn solve_n(&mut self, limit: usize) -> Vec<AnswerSet> {
        let mut answer_sets = Vec::new();
        self.solve_streaming(limit, |answer_set| {
            answer_sets.push(answer_set);
        });
        answer_sets
    }

    /// Find stable models, calling the callback for each one as it's found.
    ///
    /// Uses a solution callback to check minimality inline during search.
    /// When the candidate solver finds an assignment, the callback runs the
    /// check solver. If an unfounded set is found, the callback returns a
    /// loop constraint which triggers conflict analysis and backtracking.
    /// If the candidate is minimal (a stable model), the callback returns None
    /// and the solver returns SAT.
    pub fn solve_streaming<F>(&mut self, limit: usize, mut on_answer: F)
    where
        F: FnMut(AnswerSet),
    {
        let start = Instant::now();
        let mut count = 0usize;

        // Create candidate solver - if trivially UNSAT, no models exist
        let Some(mut cand_solver) = self.create_cand_solver() else {
            eprintln!("c asp: 0.000s models=0 loop_constraints=0 (trivially unsat)");
            return;
        };

        // Create check solver - if trivially UNSAT, all candidates are minimal
        let mut check_solver_opt = self.create_check_solver();

        // Clone data for the callback (callback must be 'static)
        let layout = self.encoded.layout.clone();
        let program = self.program.clone();
        let cand_clauses = self.encoded.cand_clauses.clone();
        let cand_pb_constraints = self.encoded.cand_pb_constraints.clone();
        let check_clauses = self.encoded.check_clauses.clone();
        let check_pb_constraints = self.encoded.check_pb_constraints.clone();
        let mut rng = StdRng::seed_from_u64(self.rng.random());

        // Counters for stats (shared with callback)
        let loop_constraints_added = Rc::new(Cell::new(0u64));

        // Set up callback: check minimality, return loop constraint if unfounded
        let counter_clone = loop_constraints_added.clone();

        cand_solver.set_solution_callback(move |assignment: &Assignment| {
            Self::solution_callback(
                assignment,
                &layout,
                &program,
                &cand_clauses,
                &cand_pb_constraints,
                &check_clauses,
                &check_pb_constraints,
                &mut check_solver_opt,
                &mut rng,
                &counter_clone,
            )
        });

        // Outer loop: solve, emit answers, add blocking clauses
        loop {
            // NECESSARY: limit=0 means unlimited, limit>0 means stop after that many
            if limit > 0 && count >= limit {
                break;
            }

            let result = cand_solver.solve();

            if result != SolveResult::Sat {
                break; // No more models
            }

            // Got a stable model - extract answer set
            let cand_assignment = self.extract_cand_assignment(&cand_solver);
            let answer_set = self.extract_answer_set(&cand_assignment);
            on_answer(answer_set);
            count += 1;

            // Add blocking clause to prevent finding same model again
            let blocking = self.create_blocking_clause(&cand_assignment);
            if cand_solver.add_clause(&blocking).is_err() {
                break; // No more models possible
            }
        }

        let total = start.elapsed();
        let loops = loop_constraints_added.get();
        eprintln!(
            "c asp: {:.3}s models={} loop_constraints={}",
            total.as_secs_f64(),
            count,
            loops,
        );
    }

    fn solution_callback(
        assignment: &Assignment,
        layout: &VarLayout,
        program: &Program,
        cand_clauses: &Vec<Clause>,
        cand_pb_constraints: &Vec<PBConstraint>,
        check_clauses: &Vec<Clause>,
        check_pb_constraints: &Vec<PBConstraint>,
        check_solver_opt: &mut Option<Solver>,
        rng: &mut StdRng,
        counter_clone: &Rc<Cell<u64>>,
    ) -> Option<ViolatedConstraint> {
        // Extract candidate assignment from the callback's Assignment view
        let cand_assignment = Self::extract_cand_assignment_from_callback(&layout, assignment);

        // VERIFY: Check that the assignment actually satisfies all cand constraints
        // A literal is true only if assigned to the right value (unassigned = unknown, not satisfied)
        let cand_lit_is_true = |lit: i32| -> bool {
            let var = lit.abs();
            match assignment.get_value(var) {
                Some(true) => lit > 0,
                Some(false) => lit < 0,
                None => false, // unassigned doesn't satisfy
            }
        };
        for (clause_idx, clause) in cand_clauses.iter().enumerate() {
            let satisfied = clause.iter().any(|&lit| cand_lit_is_true(lit));
            if !satisfied {
                eprintln!(
                    "BUG: Cand assignment does not satisfy clause {}: {:?}",
                    clause_idx, clause
                );
                eprintln!("  Literal values:");
                for &lit in clause {
                    let var = lit.abs();
                    let val = assignment.get_value(var);
                    eprintln!("    lit {} (var {}) = {:?}", lit, var, val);
                }
                panic!("Assignment from callback does not satisfy candidate clauses!");
            }
        }
        for (pb_idx, pb) in cand_pb_constraints.iter().enumerate() {
            let sum: i32 = pb
                .terms
                .iter()
                .map(
                    |&(lit, weight)| {
                        if cand_lit_is_true(lit) { weight } else { 0 }
                    },
                )
                .sum();
            if sum < pb.bound {
                eprintln!(
                    "BUG: Cand assignment does not satisfy PB constraint {}: sum={} < bound={}",
                    pb_idx, sum, pb.bound
                );
                eprintln!("  Constraint kind: {}", pb.kind);
                eprintln!("  Terms:");
                for &(lit, weight) in &pb.terms {
                    let var = lit.abs();
                    let val = assignment.get_value(var);
                    eprintln!(
                        "    lit {} (var {}) weight {} = {:?}",
                        lit, var, weight, val
                    );
                }
                panic!("Assignment from callback does not satisfy candidate PB constraints!");
            }
        }

        // If check solver is None, all candidates are minimal (no smaller model can exist)
        let Some(check_solver) = check_solver_opt else {
            return None; // Accept as stable model
        };

        // Set externals on check solver
        let externals: Vec<i32> = cand_assignment
            .iter()
            .map(|(&var, &value)| if value { var } else { -var })
            .collect();

        // Clone before passing to C++ code (can't trust it won't modify memory)
        let externals_snapshot = externals.clone();

        check_solver
            .set_externals(&externals)
            .expect("Invalid assumption");
        let result = check_solver.solve();
        check_solver.clear_externals();

        match result {
            SolveResult::Sat => {
                // VERIFY: Check that externals are respected
                for &assumption_lit in &externals_snapshot {
                    let var = assumption_lit.abs();
                    let expected = assumption_lit > 0;
                    let actual = check_solver.get_value(var);
                    if actual != Some(expected) {
                        panic!(
                            "BUG: Check solver violated assumption! var={} expected={} actual={:?}",
                            var, expected, actual
                        );
                    }
                }

                // VERIFY: Check that the check solver's assignment satisfies check constraints
                let check_lit_is_true = |lit: i32| -> bool {
                    let var = lit.abs();
                    match check_solver.get_value(var) {
                        Some(true) => lit > 0,
                        Some(false) => lit < 0,
                        None => false, // unassigned doesn't satisfy
                    }
                };
                for (clause_idx, clause) in check_clauses.iter().enumerate() {
                    let satisfied = clause.iter().any(|&lit| check_lit_is_true(lit));
                    if !satisfied {
                        eprintln!(
                            "BUG: Check solver does not satisfy check clause {}: {:?}",
                            clause_idx, clause
                        );
                        eprintln!("  Literal values:");
                        for &lit in clause {
                            let var = lit.abs();
                            let val = check_solver.get_value(var);
                            eprintln!("    lit {} (var {}) = {:?}", lit, var, val);
                        }
                        panic!("Check solver assignment does not satisfy check clauses!");
                    }
                }
                for (pb_idx, pb) in check_pb_constraints.iter().enumerate() {
                    let sum: i32 = pb
                        .terms
                        .iter()
                        .map(
                            |&(lit, weight)| {
                                if check_lit_is_true(lit) { weight } else { 0 }
                            },
                        )
                        .sum();
                    if sum < pb.bound {
                        eprintln!(
                            "BUG: Check solver does not satisfy check PB constraint {}: sum={} < bound={}",
                            pb_idx, sum, pb.bound
                        );
                        eprintln!("  Constraint kind: {}", pb.kind);
                        eprintln!("  Terms:");
                        for &(lit, weight) in &pb.terms {
                            let var = lit.abs();
                            let val = check_solver.get_value(var);
                            eprintln!(
                                "    lit {} (var {}) weight {} = {:?}",
                                lit, var, weight, val
                            );
                        }
                        panic!("Check solver assignment does not satisfy check PB constraints!");
                    }
                }

                // Found smaller model - extract unfounded set
                let unfounded_set =
                    extract_unfounded_set_standalone(&layout, &cand_assignment, &check_solver);

                // DEBUG: Verify unfounded set completeness
                let ufs_set: std::collections::HashSet<Atom> =
                    unfounded_set.iter().copied().collect();
                for &ufs_atom in &unfounded_set {
                    for entry in layout.rules_for_head(ufs_atom) {
                        let rule = &program.rules[entry.rule_idx as usize];
                        let body = match rule {
                            crate::types::Rule::Choice(r) => &r.body,
                            crate::types::Rule::Disjunctive(r) => &r.body,
                        };
                        for lit in body {
                            if lit.positive {
                                let cand_var = layout.cand(lit.atom);
                                let check_var = layout.check(lit.atom);
                                let in_cand = cand_assignment
                                    .get(&cand_var)
                                    .copied()
                                    .expect("cand_var should have value");
                                let in_check = check_solver
                                    .get_value(check_var)
                                    .expect("check_var should have value");
                                let in_ufs = ufs_set.contains(&lit.atom);
                                if in_cand && !in_check && !in_ufs {
                                    eprintln!(
                                        "BUG: Atom({}) is true in cand, false in check, but NOT in unfounded set!",
                                        lit.atom.0
                                    );
                                    eprintln!("  cand_var={}, check_var={}", cand_var, check_var);
                                    eprintln!("  in_cand={}, in_check={}", in_cand, in_check);
                                    panic!("Unfounded set is incomplete!");
                                }
                            }
                        }
                    }
                }

                // Pick random atom from unfounded set
                let idx = rng.random_range(0..unfounded_set.len());
                let chosen_atom = unfounded_set[idx];

                // Generate loop constraint
                let pb = generate_loop_constraint(
                    chosen_atom,
                    &unfounded_set,
                    &program,
                    &layout,
                    |var| cand_assignment.get(&var).copied(),
                    |var| check_solver.get_value(var),
                    rng,
                    false, // not initial setup - expect variables to be assigned
                    Some(&cand_pb_constraints),
                    Some(&check_pb_constraints),
                );

                counter_clone.update(|x| x + 1);

                // Return violated constraint
                let (lits, coefs): (Vec<i32>, Vec<i32>) = pb.terms.iter().copied().unzip();
                Some(ViolatedConstraint {
                    lits,
                    coefs,
                    rhs: pb.bound as i64,
                })
            }
            SolveResult::Unsat => {
                // Minimal - this is a stable model, accept it
                None
            }
            e @ (SolveResult::Inconsistent | SolveResult::Unknown) => {
                panic!("Check solver returned {e:?}")
            }
        }
    }

    /// Create the candidate solver with base constraints.
    /// Returns None if the problem is trivially UNSAT (no models exist).
    fn create_cand_solver(&self) -> Option<Solver> {
        let layout = &self.encoded.layout;
        let num_vars = layout.total_vars();

        let mut solver = Solver::new().expect("Failed to create solver");
        solver.set_num_vars(num_vars as i32);

        // Add base candidate clauses
        for clause in &self.encoded.cand_clauses {
            match solver.add_clause(clause) {
                Ok(()) => {}
                Err(SolverError::UnsatAtRoot) => return None,
                Err(e) => panic!("Failed to add clause: {}", e),
            }
        }

        // Add base candidate PB constraints
        for pb in &self.encoded.cand_pb_constraints {
            let (lits, coefs): (Vec<i32>, Vec<i32>) = pb.terms.iter().copied().unzip();
            match solver.add_pb_constraint(&lits, &coefs, pb.bound as i64) {
                Ok(()) => {}
                Err(SolverError::UnsatAtRoot) => return None,
                Err(e) => panic!("Failed to add PB constraint: {}", e),
            }
        }

        Some(solver)
    }

    /// Create the check solver with base constraints.
    /// Returns None if trivially UNSAT (no smaller model can exist - all candidates are minimal).
    fn create_check_solver(&self) -> Option<Solver> {
        let layout = &self.encoded.layout;
        let num_vars = layout.total_vars();

        let mut solver = Solver::new().expect("Failed to create solver");
        solver.set_num_vars(num_vars as i32);

        // Add check clauses
        for clause in &self.encoded.check_clauses {
            match solver.add_clause(clause) {
                Ok(()) => {}
                Err(SolverError::UnsatAtRoot) => return None,
                Err(e) => panic!("Failed to add check clause: {}", e),
            }
        }

        // Add check PB constraints
        for pb in &self.encoded.check_pb_constraints {
            let (lits, coefs): (Vec<i32>, Vec<i32>) = pb.terms.iter().copied().unzip();
            match solver.add_pb_constraint(&lits, &coefs, pb.bound as i64) {
                Ok(()) => {}
                Err(SolverError::UnsatAtRoot) => return None,
                Err(e) => panic!("Failed to add check PB constraint: {}", e),
            }
        }

        Some(solver)
    }

    /// Extract candidate assignment from solver.
    fn extract_cand_assignment(&self, solver: &Solver) -> HashMap<Var, bool> {
        let layout = &self.encoded.layout;
        let mut assignment = HashMap::new();

        // Extract atom values
        for atom_id in 1..=layout.num_atoms {
            let var = layout.cand(Atom(atom_id));
            if let Some(value) = solver.get_value(var) {
                assignment.insert(var, value);
            }
        }

        // Extract active rule values at all levels
        for rule_idx in 0..layout.num_rules {
            let num_levels = layout.num_levels(rule_idx);
            let bound = layout.bound(rule_idx);
            for level_offset in 0..num_levels {
                let level = bound + level_offset as i32;
                let var = layout.active_cand(rule_idx, level);
                if let Some(value) = solver.get_value(var) {
                    assignment.insert(var, value);
                }
            }
        }

        assignment
    }

    /// Extract candidate assignment from Assignment (callback view).
    fn extract_cand_assignment_from_callback(
        layout: &crate::encoding::VarLayout,
        assignment: &Assignment,
    ) -> HashMap<Var, bool> {
        let mut result = HashMap::new();

        // Extract atom values
        for atom_id in 1..=layout.num_atoms {
            let var = layout.cand(Atom(atom_id));
            if let Some(value) = assignment.get_value(var) {
                result.insert(var, value);
            }
        }

        // Extract active rule values at all levels
        for rule_idx in 0..layout.num_rules {
            let num_levels = layout.num_levels(rule_idx);
            let bound = layout.bound(rule_idx);
            for level_offset in 0..num_levels {
                let level = bound + level_offset as i32;
                let var = layout.active_cand(rule_idx, level);
                if let Some(value) = assignment.get_value(var) {
                    result.insert(var, value);
                }
            }
        }

        result
    }

    /// Create a blocking clause to prevent finding the same candidate.
    fn create_blocking_clause(&self, cand_assignment: &HashMap<Var, bool>) -> Clause {
        let layout = &self.encoded.layout;
        let mut clause = Vec::new();

        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let var = layout.cand(atom);
            if let Some(&value) = cand_assignment.get(&var) {
                if value {
                    clause.push(neg(var));
                } else {
                    clause.push(pos(var));
                }
            }
            // If neither, atom is unassigned - don't include in blocking clause
        }

        clause
    }

    /// Extract the answer set from the candidate assignment.
    fn extract_answer_set(&self, cand_assignment: &HashMap<Var, bool>) -> AnswerSet {
        let layout = &self.encoded.layout;
        let mut answer_set = HashSet::default();

        // Extract true atoms with symbol names (non-auxiliary atoms)
        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let var = layout.cand(atom);
            if let Some(&value) = cand_assignment.get(&var)
                && value
                && self.is_shown_atom(atom)
            {
                answer_set.insert(atom);
            }
        }

        answer_set
    }

    /// Check if an atom should be shown in output.
    fn is_shown_atom(&self, atom: Atom) -> bool {
        self.atom_names.contains_key(&atom)
    }

    /// Get atom name from symbol table.
    pub fn atom_name(&self, atom: Atom) -> Option<&str> {
        self.atom_names.get(&atom).map(|s| s.as_str())
    }

    /// Get a clone of the atom names map for use outside the solver.
    pub fn atom_names(&self) -> HashMap<Atom, String> {
        self.atom_names.clone()
    }
}

#[cfg(test)]
mod tests;
