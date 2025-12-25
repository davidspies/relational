//! ASP solver using RoundingSat PB solver.
//!
//! Architecture:
//! - Candidate solver: finds candidate answer sets
//! - Check solver: checks if a strictly smaller model exists (unfounded set detection)
//! - Both solvers are created once; constraints are added incrementally to cand_solver,
//!   and check_solver uses assumptions for the candidate assignment.

use std::collections::HashMap;
use std::time::Instant;

use contiguous_data::HashSet;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use roundingsat::{SolveResult, Solver};

use crate::encoding::{Clause, EncodedProgram, Var, neg, pos};
use crate::encoding::{encode_program, generate_loop_constraint};
use crate::types::{Atom, Program};

/// An answer set (stable model).
pub type AnswerSet = HashSet<Atom>;

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
    pub fn solve_streaming<F>(&mut self, limit: usize, mut on_answer: F)
    where
        F: FnMut(AnswerSet),
    {
        use std::time::Duration;
        const STATS_INTERVAL: Duration = Duration::from_secs(5);

        let start = Instant::now();
        let mut last_stats = Instant::now();
        let mut count = 0usize;
        let mut cand_calls = 0u64;
        let mut check_calls = 0u64;
        let mut loop_constraints_added = 0u64;

        // Timing metrics
        let mut cand_solve_time = Duration::ZERO;
        let mut check_solve_time = Duration::ZERO;

        // Create both solvers once
        let mut cand_solver = self.create_cand_solver();
        let mut check_solver = self.create_check_solver();

        loop {
            // NECESSARY: limit=0 means unlimited, limit>0 means stop after that many
            if limit > 0 && count >= limit {
                break;
            }

            // Periodic stats for long solves
            if last_stats.elapsed() >= STATS_INTERVAL {
                let total = start.elapsed();
                let rs_time = cand_solve_time + check_solve_time;
                eprintln!(
                    "c asp: {:.1}s models={} cand={} check={} loops={} | roundingsat={:.1}s ({:.0}%) other={:.1}s ({:.0}%)",
                    total.as_secs_f64(),
                    count,
                    cand_calls,
                    check_calls,
                    loop_constraints_added,
                    rs_time.as_secs_f64(),
                    100.0 * rs_time.as_secs_f64() / total.as_secs_f64(),
                    (total - rs_time).as_secs_f64(),
                    100.0 * (total - rs_time).as_secs_f64() / total.as_secs_f64(),
                );
                last_stats = Instant::now();
            }

            // Step 1: Solve candidate solver
            let solve_start = Instant::now();
            let result = cand_solver.solve();
            cand_solve_time += solve_start.elapsed();
            cand_calls += 1;

            if result != SolveResult::Sat {
                break; // No more candidates
            }

            // Get candidate assignment
            let cand_assignment = self.extract_cand_assignment(&cand_solver);

            // Step 2: Check if this candidate is a stable model
            match self.check_minimality(
                &cand_assignment,
                &mut check_solver,
                &mut check_calls,
                &mut check_solve_time,
            ) {
                MinimalityResult::IsMinimal => {
                    // Found a stable model!
                    let answer_set = self.extract_answer_set(&cand_assignment);
                    on_answer(answer_set);
                    count += 1;

                    // Add blocking clause to prevent finding same model again
                    let blocking = self.create_blocking_clause(&cand_assignment);
                    if cand_solver.add_clause(&blocking).is_err() {
                        break; // No more models possible
                    }
                }
                MinimalityResult::SmallerExists { unfounded_set } => {
                    // Pick a random atom from the unfounded set to assert
                    // (like clasp, we process one atom at a time)
                    let idx = self.rng.random_range(0..unfounded_set.len());
                    let chosen_atom = unfounded_set[idx];

                    // Not stable - add loop constraint
                    // Use the current assignment to detect stealing heads
                    let (terms, bound) = generate_loop_constraint(
                        chosen_atom,
                        &unfounded_set,
                        &self.program,
                        &self.encoded.layout,
                        &cand_assignment,
                        &mut self.rng,
                    );

                    loop_constraints_added += 1;

                    // Add loop constraint to candidate solver
                    let (lits, coefs): (Vec<i32>, Vec<i32>) = terms.iter().copied().unzip();
                    if cand_solver
                        .add_pb_constraint(&lits, &coefs, bound as i64)
                        .is_err()
                    {
                        break; // No more models possible
                    }
                }
            }
        }

        let total = start.elapsed();
        let rs_time = cand_solve_time + check_solve_time;
        eprintln!(
            "c asp: {:.3}s models={} cand_calls={} check_calls={} loop_constraints={}",
            total.as_secs_f64(),
            count,
            cand_calls,
            check_calls,
            loop_constraints_added,
        );
        eprintln!(
            "c timing: roundingsat={:.3}s ({:.1}%) cand={:.3}s check={:.3}s | other={:.3}s ({:.1}%)",
            rs_time.as_secs_f64(),
            100.0 * rs_time.as_secs_f64() / total.as_secs_f64(),
            cand_solve_time.as_secs_f64(),
            check_solve_time.as_secs_f64(),
            (total - rs_time).as_secs_f64(),
            100.0 * (total - rs_time).as_secs_f64() / total.as_secs_f64(),
        );
    }

    /// Create the candidate solver with base constraints.
    fn create_cand_solver(&self) -> Solver {
        let layout = &self.encoded.layout;
        let num_vars = layout.total_vars();

        let mut solver = Solver::new().expect("Failed to create solver");
        solver.set_num_vars(num_vars as i32);

        // Add base candidate clauses
        for clause in &self.encoded.cand_clauses {
            let _ = solver.add_clause(clause);
        }

        // Add base candidate PB constraints
        for (terms, bound) in &self.encoded.cand_pb_constraints {
            let (lits, coefs): (Vec<i32>, Vec<i32>) = terms.iter().copied().unzip();
            let _ = solver.add_pb_constraint(&lits, &coefs, *bound as i64);
        }

        solver
    }

    /// Create the check solver with base constraints.
    fn create_check_solver(&self) -> Solver {
        let layout = &self.encoded.layout;
        let num_vars = layout.total_vars();

        let mut solver = Solver::new().expect("Failed to create solver");
        solver.set_num_vars(num_vars as i32);

        // Add check clauses
        for clause in &self.encoded.check_clauses {
            let _ = solver.add_clause(clause);
        }

        // Add check PB constraints
        for (terms, bound) in &self.encoded.check_pb_constraints {
            let (lits, coefs): (Vec<i32>, Vec<i32>) = terms.iter().copied().unzip();
            let _ = solver.add_pb_constraint(&lits, &coefs, *bound as i64);
        }

        solver
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

    /// Check if the candidate is minimal (no unfounded set exists).
    fn check_minimality(
        &self,
        cand_assignment: &HashMap<Var, bool>,
        check_solver: &mut Solver,
        check_calls: &mut u64,
        check_solve_time: &mut std::time::Duration,
    ) -> MinimalityResult {
        // Set candidate assignment as assumptions
        let assumptions: Vec<i32> = cand_assignment
            .iter()
            .map(|(&var, &value)| if value { var } else { -var })
            .collect();
        check_solver
            .set_assumptions(&assumptions)
            .expect("Invalid assumption");

        let solve_start = Instant::now();
        let result = check_solver.solve();
        *check_solve_time += solve_start.elapsed();
        *check_calls += 1;

        // Clear assumptions for next call
        check_solver.clear_assumptions();

        if result == SolveResult::Sat {
            // Found a smaller model - extract unfounded set
            let unfounded_set = self.extract_unfounded_set(cand_assignment, check_solver);
            MinimalityResult::SmallerExists { unfounded_set }
        } else {
            MinimalityResult::IsMinimal
        }
    }

    /// Extract unfounded set: atoms in candidate but not in check.
    fn extract_unfounded_set(
        &self,
        cand_assignment: &HashMap<Var, bool>,
        check_solver: &Solver,
    ) -> Vec<Atom> {
        let layout = &self.encoded.layout;
        let mut unfounded = Vec::new();

        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let cand_var = layout.cand(atom);
            let check_var = layout.check(atom);

            let in_cand = cand_assignment.get(&cand_var).copied().unwrap_or(false);
            let in_check = check_solver.get_value(check_var).unwrap_or(false);

            if in_cand && !in_check {
                unfounded.push(atom);
            }
        }

        unfounded
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

/// Result of the minimality check.
enum MinimalityResult {
    /// The candidate is minimal (no unfounded set exists).
    IsMinimal,
    /// An unfounded set exists (strictly smaller model found).
    SmallerExists {
        /// Atoms that are in candidate but not in check (the unfounded set).
        unfounded_set: Vec<Atom>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_smodels;

    /// Helper to solve an ASP program and return sorted answer set names.
    fn solve_asp(input: &str) -> Vec<Vec<String>> {
        let program = parse_smodels(input).unwrap();
        let mut solver = AspSolver::new(program);
        let mut results: Vec<Vec<String>> = solver
            .solve()
            .into_iter()
            .map(|answer_set| {
                let mut names: Vec<String> = answer_set
                    .iter()
                    .filter_map(|atom| solver.atom_name(*atom).map(String::from))
                    .collect();
                names.sort();
                names
            })
            .collect();
        results.sort();
        results
    }

    #[test]
    fn test_simple_fact() {
        // p.
        let input = "1 2 0 0\n0\n2 p\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["p"]]);
    }

    #[test]
    fn test_two_facts() {
        // p. q.
        let input = "1 2 0 0\n1 3 0 0\n0\n2 p\n3 q\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["p", "q"]]);
    }

    #[test]
    fn test_default_negation_two_models() {
        // a :- not b. b :- not a.
        // Two answer sets: {a} and {b}
        let input = "1 2 1 1 3\n1 3 1 1 2\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"], vec!["b"]]);
    }

    #[test]
    fn test_self_referential_negation_unsat() {
        // a :- not a.
        // No stable models
        let input = "1 2 1 1 2\n0\n2 a\n0\n";
        let results = solve_asp(input);
        assert!(results.is_empty());
    }

    #[test]
    fn test_constraint_filters_model() {
        // a :- not b. b :- not a. :- a, b.
        // Still two answer sets since a and b can't both be true anyway
        let input = "1 2 1 1 3\n1 3 1 1 2\n1 1 2 0 2 3\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"], vec!["b"]]);
    }

    #[test]
    fn test_constraint_makes_unsat() {
        // a :- not b. b :- not a. :- a. :- b.
        // No stable models - both a and b are forbidden
        let input = "1 2 1 1 3\n1 3 1 1 2\n1 1 1 0 2\n1 1 1 0 3\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert!(results.is_empty());
    }

    #[test]
    fn test_chain_derivation() {
        // a. b :- a. c :- b.
        // One answer set: {a, b, c}
        let input = "1 2 0 0\n1 3 1 0 2\n1 4 1 0 3\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a", "b", "c"]]);
    }

    #[test]
    fn test_unfounded_loop() {
        // a :- b. b :- a.
        // No facts, so no stable models with a or b (only empty model)
        let input = "1 2 1 0 3\n1 3 1 0 2\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![Vec::<String>::new()]);
    }

    #[test]
    fn test_empty_program() {
        // Empty program has one answer set: {}
        let input = "0\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![Vec::<String>::new()]);
    }

    #[test]
    fn test_supported_loop() {
        // a :- b. b :- a. a :- not c.
        // c is false by default, so a is supported, then b is supported
        let input = "1 2 1 0 3\n1 3 1 0 2\n1 2 1 1 4\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a", "b"]]);
    }

    #[test]
    fn test_constraint_requires_derivation() {
        // a. :- not a. (a is a fact, constraint requires a to be true - satisfied)
        let input = "1 2 0 0\n1 1 1 1 2\n0\n2 a\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"]]);
    }

    #[test]
    fn test_multiple_rules_same_head() {
        // a :- b. a :- c. b.
        // a is derived from b
        let input = "1 2 1 0 3\n1 2 1 0 4\n1 3 0 0\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a", "b"]]);
    }

    #[test]
    fn test_choice_rule_forbidden() {
        // {a}. :- a.
        // Only {} is valid since a is forbidden
        let input = "3 1 2 0 0\n1 1 1 0 2\n0\n2 a\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![Vec::<String>::new()]);
    }

    #[test]
    fn test_choice_rule_required() {
        // {a}. :- not a.
        // Only {a} is valid since a is required
        let input = "3 1 2 0 0\n1 1 1 1 2\n0\n2 a\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"]]);
    }

    #[test]
    fn test_disjunctive_simple() {
        // a | b.
        // Two answer sets: {a} and {b}
        // Format: 8 num_heads h1 h2 num_pos num_neg body_lits...
        let input = "8 2 2 3 0 0\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"], vec!["b"]]);
    }

    #[test]
    fn test_disjunctive_with_body() {
        // a | b :- c. c.
        // Two answer sets: {a, c} and {b, c}
        // Rule 1: 8 2 2 3 1 0 4 (a | b :- c)
        // Rule 2: 1 4 0 0 (c.)
        let input = "8 2 2 3 1 0 4\n1 4 0 0\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a", "c"], vec!["b", "c"]]);
    }

    #[test]
    fn test_disjunctive_with_constraint() {
        // a | b. :- a.
        // Only {b} is valid since a is forbidden
        let input = "8 2 2 3 0 0\n1 1 1 0 2\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["b"]]);
    }

    #[test]
    fn test_disjunctive_three_heads() {
        // a | b | c.
        // Three answer sets: {a}, {b}, {c}
        let input = "8 3 2 3 4 0 0\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"], vec!["b"], vec!["c"]]);
    }

    #[test]
    fn test_disjunctive_with_default_negation() {
        // a | b. c :- not a.
        // Two answer sets: {a} and {b, c}
        let input = "8 2 2 3 0 0\n1 4 1 1 2\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"], vec!["b", "c"]]);
    }

    #[test]
    fn test_weight_body_loop() {
        // a :- #count{b:b; c:c} >= 1.
        // b :- a.
        // {c}.
        // :- not c.
        // Expected answer set: {a, b, c}
        //
        // From gringo --output=smodels:
        // 3 1 2 0 0       -> {c}.  (choice rule, head=2)
        // 1 1 1 1 2       -> :- not c.  (constraint: false :- not c)
        // 1 3 1 0 4       -> a :- aux4
        // 1 5 1 0 3       -> b :- a
        // 2 6 2 0 1 2 5   -> aux6 :- {c, b} >= 1  (cardinality rule)
        // 1 4 1 0 6       -> aux4 :- aux6
        // Symbols: 2=c, 3=a, 5=b
        let input = "3 1 2 0 0\n1 1 1 1 2\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a", "b", "c"]]);
    }

    #[test]
    fn test_weight_body_loop_empty() {
        // a :- #count{b:b; c:c} >= 1.
        // b :- a.
        // {c}.
        // :- c.       (c must be false)
        //
        // The only stable model is {} because:
        // - c=false (from :- c)
        // - a requires {b, c} >= 1, but c=false so need b=true
        // - b requires a, which requires b (circular with no external support)
        // - So {a, b} is unfounded and rejected
        //
        // From gringo --output=smodels:
        // 3 1 2 0 0       -> {c}.
        // 1 1 1 0 2       -> :- c.
        // 1 3 1 0 4       -> a :- aux4
        // 1 5 1 0 3       -> b :- a
        // 2 6 2 0 1 2 5   -> aux6 :- {c, b} >= 1
        // 1 4 1 0 6       -> aux4 :- aux6
        // Symbols: 2=c, 3=a, 5=b
        let input = "3 1 2 0 0\n1 1 1 0 2\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
        let results = solve_asp(input);
        // Only the empty set should be a stable model
        assert_eq!(
            results,
            vec![Vec::<&str>::new()],
            "Expected only empty set but got: {:?}",
            results
        );
    }

    #[test]
    fn test_weight_body_two_models() {
        // a :- #count{b:b; c:c} >= 1.
        // b :- a.
        // {c}.
        //
        // Two stable models:
        // - {} (empty: c not chosen, so a not derived, so b not derived)
        // - {a, b, c} (c chosen, {c} >= 1 satisfied, a derived, b derived)
        //
        // From gringo --output=smodels:
        // 3 1 2 0 0       -> {c}.
        // 1 3 1 0 4       -> a :- aux4
        // 1 5 1 0 3       -> b :- a
        // 2 6 2 0 1 2 5   -> aux6 :- {c, b} >= 1
        // 1 4 1 0 6       -> aux4 :- aux6
        // Symbols: 2=c, 3=a, 5=b
        let input =
            "3 1 2 0 0\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results.len(), 2, "Expected 2 models but got: {:?}", results);
        assert!(
            results.iter().any(|m| m.is_empty()),
            "Expected empty model but got: {:?}",
            results
        );
        assert!(
            results.iter().any(|m| m == &vec!["a", "b", "c"]),
            "Expected {{a,b,c}} but got: {:?}",
            results
        );
    }

    #[test]
    fn test_self_loop_rule() {
        // Program:
        //   {a}.
        //   b :- a.
        //   c :- a.
        //   d :- b, c.
        //   d :- d, d.  % Self-loop
        //   :- not d, a.
        //
        // Two stable models: {} and {a, b, c, d}
        //
        // From gringo --output=smodels:
        // 3 1 2 0 0      -> {a}.
        // 1 3 1 0 2      -> b :- a
        // 1 4 1 0 2      -> c :- a
        // 1 5 2 0 3 4    -> d :- b, c
        // 1 5 2 0 5 5    -> d :- d, d
        // 1 1 2 1 5 2    -> :- not d, a
        // Symbols: 2=a, 5=d
        let input = "3 1 2 0 0\n1 3 1 0 2\n1 4 1 0 2\n1 5 2 0 3 4\n1 5 2 0 5 5\n1 1 2 1 5 2\n0\n2 a\n5 d\n0\n";
        let results = solve_asp(input);
        assert_eq!(results.len(), 2, "Expected 2 models but got: {:?}", results);
        assert!(
            results.iter().any(|m| m.is_empty()),
            "Expected empty model but got: {:?}",
            results
        );
        assert!(
            results.iter().any(|m| m == &vec!["a", "d"]),
            "Expected {{a, d}} but got: {:?}",
            results
        );
    }

    #[test]
    fn test_loop_constraint_should_be_added_not_blocked() {
        // Program:
        //   a :- {b, c} >= 1.
        //   b :- a.
        //   {c}.
        //   :- c.       (c must be false)
        //
        // Only valid stable model is {} (empty set).

        let input = "3 1 2 0 0\n1 1 1 0 2\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
        let program = crate::parse_smodels(input).unwrap();
        let mut solver = super::AspSolver::new(program);

        let results = solver.solve();

        // The result is correct (empty set only)
        assert_eq!(results.len(), 1);
        assert!(results[0].is_empty());
    }
}
