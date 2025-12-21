//! ASP solver using two CDCL SAT solvers.
//!
//! Architecture:
//! - Candidate solver: finds candidate answer sets
//! - Check solver: checks if a strictly smaller model exists (unfounded set detection)
//! - External relation: candidate's assignments feed into check as external input
//! - Both solvers share ONE database and retain learned clauses

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;

use cdcl::{Level, Lit, Var};
use contiguous_data::{HashSet, Multiset};
use relational::create_persistent_input;
use relational::database::{Database, DatabaseBuilder};

use crate::encoding::{Clause, EncodedProgram, encode_program, generate_loop_constraint};
use crate::types::{Atom, Program};

/// Dump solver setup for replay.
fn dump_setup() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("ASP_DUMP_SETUP").is_ok())
}

/// An answer set (stable model).
pub type AnswerSet = HashSet<Atom>;

/// ASP Solver using two-solver architecture with persistent state.
pub struct AspSolver {
    program: Program,
    encoded: EncodedProgram,
    db: Database,
    cand_solver: cdcl::Solver,
    check_solver: cdcl::Solver,
    next_cand_clause_id: u32,
    /// Fast lookup: atom → symbol name
    atom_names: HashMap<Atom, String>,
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

        let mut db_builder = DatabaseBuilder::new();

        // Create empty external for candidate solver (no external assignments)
        create_persistent_input!(db_builder, _cand_external_inp, cand_external, Lit);

        // Variables for candidate solver: cand atoms + active rules
        let num_cand_vars = encoded.layout.num_atoms + encoded.layout.num_rules;
        let cand_vars: HashSet<Var> = (1..=num_cand_vars).map(Var::new).collect();
        let cand_solver = cdcl::Solver::new(&mut db_builder, &cand_vars, cand_external);

        // Check solver uses candidate's assigned literals as external input
        let check_external = cand_solver.assigned_saved().get();

        // Variables for check solver: all variables
        let check_vars: HashSet<Var> = (1..=encoded.layout.total_vars()).map(Var::new).collect();
        let check_solver = cdcl::Solver::new(&mut db_builder, &check_vars, check_external);

        let mut db = db_builder.build();
        let mut cand_solver = cand_solver;
        let mut check_solver = check_solver;

        // Add candidate clauses
        for (i, clause) in encoded.cand_clauses.iter().enumerate() {
            cand_solver.add_clause(&mut db, i as u32, clause);
        }
        let mut next_id = encoded.cand_clauses.len() as u32;

        // Add candidate PB constraints
        for (i, (terms, bound)) in encoded.cand_pb_constraints.iter().enumerate() {
            cand_solver.add_pb_constraint(&mut db, next_id + i as u32, terms, *bound);
        }
        next_id += encoded.cand_pb_constraints.len() as u32;

        // Add check clauses
        for (i, clause) in encoded.check_clauses.iter().enumerate() {
            check_solver.add_clause(&mut db, i as u32, clause);
        }

        // Add check PB constraints
        let check_pb_id_offset = encoded.check_clauses.len() as u32;
        for (i, (terms, bound)) in encoded.check_pb_constraints.iter().enumerate() {
            check_solver.add_pb_constraint(&mut db, check_pb_id_offset + i as u32, terms, *bound);
        }

        // Dump setup for replay
        if dump_setup() {
            eprintln!("// === SOLVER SETUP DUMP ===");
            eprintln!(
                "// num_cand_vars = {}",
                encoded.layout.num_atoms + encoded.layout.num_rules
            );
            eprintln!("// total_vars = {}", encoded.layout.total_vars());
            eprintln!("// Cand clauses:");
            for (i, clause) in encoded.cand_clauses.iter().enumerate() {
                let lits: Vec<i32> = clause
                    .iter()
                    .map(|lit| {
                        let v = lit.var().raw() as i32;
                        if lit.is_positive() { v } else { -v }
                    })
                    .collect();
                eprintln!("CAND_CLAUSE {} {:?}", i, lits);
            }
            eprintln!("// Cand PB constraints:");
            for (i, (terms, bound)) in encoded.cand_pb_constraints.iter().enumerate() {
                let ts: Vec<(i32, i64)> = terms
                    .iter()
                    .map(|(lit, w)| {
                        let v = lit.var().raw() as i32;
                        let v = if lit.is_positive() { v } else { -v };
                        (v, *w)
                    })
                    .collect();
                eprintln!(
                    "CAND_PB {} {:?} {}",
                    encoded.cand_clauses.len() + i,
                    ts,
                    bound
                );
            }
            eprintln!("// Check clauses:");
            for (i, clause) in encoded.check_clauses.iter().enumerate() {
                let lits: Vec<i32> = clause
                    .iter()
                    .map(|lit| {
                        let v = lit.var().raw() as i32;
                        if lit.is_positive() { v } else { -v }
                    })
                    .collect();
                eprintln!("CHECK_CLAUSE {} {:?}", i, lits);
            }
            eprintln!("// Check PB constraints:");
            for (i, (terms, bound)) in encoded.check_pb_constraints.iter().enumerate() {
                let ts: Vec<(i32, i64)> = terms
                    .iter()
                    .map(|(lit, w)| {
                        let v = lit.var().raw() as i32;
                        let v = if lit.is_positive() { v } else { -v };
                        (v, *w)
                    })
                    .collect();
                eprintln!(
                    "CHECK_PB {} {:?} {}",
                    check_pb_id_offset as usize + i,
                    ts,
                    bound
                );
            }
            eprintln!("// === END DUMP ===");
        }

        AspSolver {
            program,
            encoded,
            db,
            cand_solver,
            check_solver,
            next_cand_clause_id: next_id,
            atom_names,
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
        let start = Instant::now();
        let mut count = 0usize;
        let mut cand_calls = 0u64;
        let mut check_calls = 0u64;
        let mut loop_constraints = 0u64;

        loop {
            if limit > 0 && count >= limit {
                break;
            }

            // Step 1: Solve candidate to find a candidate answer set
            let (sat, _stats) = self.cand_solver.solve_and_stay(&mut self.db);
            cand_calls += 1;
            if !sat {
                break; // No more candidates - UNSAT
            }

            // Step 2: Check if check solver finds a strictly smaller model (unfounded set)
            match self.check_minimality(&mut check_calls) {
                MinimalityResult::IsMinimal => {
                    // Found a stable model!
                    let answer_set = self.extract_answer_set();
                    on_answer(answer_set);
                    count += 1;

                    // Add blocking clause to prevent finding same model again
                    let blocking = self.blocking_clause();
                    self.add_cand_clause(&blocking);

                    // Backtrack to try again
                    self.cand_solver.backtrack(&mut self.db, Level::TOP);
                }
                MinimalityResult::SmallerExists { unfounded_set } => {
                    // Not stable - add loop constraint
                    let assignment = self.cand_solver.get_assignment();
                    let (terms, bound) = generate_loop_constraint(
                        &unfounded_set,
                        &self.program,
                        &self.encoded.layout,
                        &assignment,
                    );

                    // Assert the loop constraint is currently violated
                    let sum: i64 = terms
                        .iter()
                        .map(|(lit, weight)| {
                            let lit_true = assignment.contains(&lit);
                            if lit_true { *weight } else { 0 }
                        })
                        .sum();
                    assert!(
                        sum < bound,
                        "Loop constraint should be violated: sum={} >= bound={}",
                        sum,
                        bound
                    );
                    drop(assignment);

                    // Debug output
                    if std::env::var("ASP_DEBUG").is_ok() {
                        let atom_ids: Vec<_> = unfounded_set.iter().map(|a| a.0).collect();
                        let names: Vec<_> = unfounded_set
                            .iter()
                            .filter_map(|a| self.atom_name(*a))
                            .collect();
                        let num_reasons = terms.iter().filter(|(_, w)| *w > 1).count();
                        eprintln!(
                            "c UFS atoms {:?} named {:?} -- {} reasons",
                            atom_ids, names, num_reasons
                        );
                    }

                    loop_constraints += 1;

                    let backtrack_level = self.compute_backtrack_level_pb(&terms);
                    self.add_cand_pb_constraint(&terms, bound);
                    self.cand_solver.backtrack(&mut self.db, backtrack_level);
                }
            }
        }

        eprintln!(
            "c asp: {:.3}s models={} cand_calls={} check_calls={} loop_constraints={}",
            start.elapsed().as_secs_f64(),
            count,
            cand_calls,
            check_calls,
            loop_constraints
        );
    }

    /// Add a clause to the candidate solver.
    fn add_cand_clause(&mut self, clause: &Clause) {
        self.cand_solver
            .add_clause(&mut self.db, self.next_cand_clause_id, clause);
        self.next_cand_clause_id += 1;
    }

    /// Add a PB constraint to the candidate solver.
    fn add_cand_pb_constraint(&mut self, terms: &[(Lit, cdcl::Weight)], bound: cdcl::Weight) {
        self.cand_solver
            .add_pb_constraint(&mut self.db, self.next_cand_clause_id, terms, bound);
        self.next_cand_clause_id += 1;
    }

    /// Compute the backtrack level for a learned PB constraint.
    fn compute_backtrack_level_pb(&self, terms: &[(Lit, cdcl::Weight)]) -> Level {
        let mut levels: Vec<Level> = terms
            .iter()
            .filter_map(|&(lit, _)| self.cand_solver.get_level(!lit))
            .collect();

        levels.sort_by(|a, b| b.cmp(a));
        levels.dedup();

        levels.get(1).copied().unwrap_or(Level::TOP)
    }

    /// Check if the current candidate assignment is minimal (no unfounded set exists).
    fn check_minimality(&mut self, check_calls: &mut u64) -> MinimalityResult {
        let (sat, _stats) = self.check_solver.solve_and_stay(&mut self.db);
        *check_calls += 1;
        if sat {
            let unfounded_set = {
                let cand_assignment = self.cand_solver.get_assignment();
                let check_assignment = self.check_solver.get_assignment();

                self.extract_unfounded_set(&cand_assignment, &check_assignment)
            };
            self.check_solver.backtrack(&mut self.db, Level::TOP);

            MinimalityResult::SmallerExists { unfounded_set }
        } else {
            MinimalityResult::IsMinimal
        }
    }

    /// Extract unfounded set: atoms in candidate but not in check (S_cand \ S_check).
    fn extract_unfounded_set(
        &self,
        cand_assignment: &Multiset<Lit>,
        check_assignment: &Multiset<Lit>,
    ) -> Vec<Atom> {
        let layout = &self.encoded.layout;
        let mut diff = Vec::new();

        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let cand_var = layout.cand(atom);
            let check_var = layout.check(atom);

            let in_cand = cand_assignment.contains(&Lit::pos(cand_var));
            let in_check = check_assignment.contains(&Lit::pos(check_var));

            if in_cand && !in_check {
                diff.push(atom);
            }
        }

        diff
    }

    /// Create a blocking clause to prevent finding the same candidate model again.
    fn blocking_clause(&self) -> Clause {
        let layout = &self.encoded.layout;
        let assignment = self.cand_solver.get_assignment();
        let mut clause = Vec::new();

        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let var = layout.cand(atom);
            if assignment.contains(&Lit::pos(var)) {
                clause.push(Lit::neg(var));
            } else if assignment.contains(&Lit::neg(var)) {
                clause.push(Lit::pos(var));
            } else {
                panic!("Atom {} not assigned in blocking clause generation", atom.0);
            }
        }

        clause
    }

    /// Extract the answer set from the current candidate assignment.
    fn extract_answer_set(&self) -> AnswerSet {
        let layout = &self.encoded.layout;
        let assignment = self.cand_solver.get_assignment();
        let mut answer_set = HashSet::default();

        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let var = layout.cand(atom);
            if assignment.contains(&Lit::pos(var)) && self.is_shown_atom(atom) {
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
    IsMinimal,
    SmallerExists { unfounded_set: Vec<Atom> },
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
        let input = "1 2 0 0\n0\n2 p\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["p"]]);
    }

    #[test]
    fn test_two_facts() {
        let input = "1 2 0 0\n1 3 0 0\n0\n2 p\n3 q\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["p", "q"]]);
    }

    #[test]
    fn test_default_negation_two_models() {
        let input = "1 2 1 1 3\n1 3 1 1 2\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"], vec!["b"]]);
    }

    #[test]
    fn test_self_referential_negation_unsat() {
        let input = "1 2 1 1 2\n0\n2 a\n0\n";
        let results = solve_asp(input);
        assert!(results.is_empty());
    }

    #[test]
    fn test_constraint_filters_model() {
        let input = "1 2 1 1 3\n1 3 1 1 2\n1 1 2 0 2 3\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"], vec!["b"]]);
    }

    #[test]
    fn test_constraint_makes_unsat() {
        let input = "1 2 1 1 3\n1 3 1 1 2\n1 1 1 0 2\n1 1 1 0 3\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert!(results.is_empty());
    }

    #[test]
    fn test_chain_derivation() {
        let input = "1 2 0 0\n1 3 1 0 2\n1 4 1 0 3\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a", "b", "c"]]);
    }

    #[test]
    fn test_unfounded_loop() {
        let input = "1 2 1 0 3\n1 3 1 0 2\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![Vec::<String>::new()]);
    }

    #[test]
    fn test_empty_program() {
        let input = "0\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![Vec::<String>::new()]);
    }

    #[test]
    fn test_supported_loop() {
        let input = "1 2 1 0 3\n1 3 1 0 2\n1 2 1 1 4\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a", "b"]]);
    }

    #[test]
    fn test_constraint_requires_derivation() {
        let input = "1 2 0 0\n1 1 1 1 2\n0\n2 a\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"]]);
    }

    #[test]
    fn test_multiple_rules_same_head() {
        let input = "1 2 1 0 3\n1 2 1 0 4\n1 3 0 0\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a", "b"]]);
    }

    #[test]
    fn test_choice_rule_forbidden() {
        let input = "3 1 2 0 0\n1 1 1 0 2\n0\n2 a\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![Vec::<String>::new()]);
    }

    #[test]
    fn test_choice_rule_required() {
        let input = "3 1 2 0 0\n1 1 1 1 2\n0\n2 a\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"]]);
    }

    #[test]
    fn test_disjunctive_simple() {
        let input = "8 2 2 3 0 0\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"], vec!["b"]]);
    }

    #[test]
    fn test_disjunctive_with_body() {
        let input = "8 2 2 3 1 0 4\n1 4 0 0\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a", "c"], vec!["b", "c"]]);
    }

    #[test]
    fn test_disjunctive_with_constraint() {
        let input = "8 2 2 3 0 0\n1 1 1 0 2\n0\n2 a\n3 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["b"]]);
    }

    #[test]
    fn test_disjunctive_three_heads() {
        let input = "8 3 2 3 4 0 0\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"], vec!["b"], vec!["c"]]);
    }

    #[test]
    fn test_disjunctive_with_default_negation() {
        let input = "8 2 2 3 0 0\n1 4 1 1 2\n0\n2 a\n3 b\n4 c\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a"], vec!["b", "c"]]);
    }

    #[test]
    fn test_weight_body_loop() {
        let input = "3 1 2 0 0\n1 1 1 1 2\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(results, vec![vec!["a", "b", "c"]]);
    }

    #[test]
    fn test_weight_body_loop_empty() {
        let input = "3 1 2 0 0\n1 1 1 0 2\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
        let results = solve_asp(input);
        assert_eq!(
            results,
            vec![Vec::<&str>::new()],
            "Expected only empty set but got: {:?}",
            results
        );
    }

    #[test]
    fn test_weight_body_two_models() {
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
}
