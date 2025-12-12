//! ASP solver using two CDCL SAT solvers.
//!
//! Architecture:
//! - Bottom solver: finds candidate models using bottom clauses
//! - Top solver: checks if a strictly smaller model exists using top clauses
//! - External relation: bottom's assignments feed into top as external input
//! - Both solvers share ONE database and retain learned clauses

use std::time::Instant;

use cdcl::{Level, Lit, Var};
use contiguous_data::HashSet;
use relational::create_persistent_input;
use relational::database::{Database, DatabaseBuilder};

use crate::encoding::{Clause, EncodedProgram, encode_program, generate_loop_constraint};
use crate::types::{Atom, Program};

/// An answer set (stable model).
pub type AnswerSet = HashSet<Atom>;

/// ASP Solver using two-solver architecture with persistent state.
pub struct AspSolver {
    program: Program,
    encoded: EncodedProgram,
    db: Database,
    bottom_solver: cdcl::Solver,
    top_solver: cdcl::Solver,
    next_bottom_clause_id: u32,
}

impl AspSolver {
    /// Create a new ASP solver for the given program.
    pub fn new(program: Program) -> Self {
        let encoded = encode_program(&program);

        let mut db_builder = DatabaseBuilder::new();

        // Create empty external for bottom solver (no external assignments)
        create_persistent_input!(db_builder, _bottom_external_inp, bottom_external, Lit);

        // Variables for bottom solver: bottom atoms + active rules
        let num_bottom_vars = encoded.layout.num_atoms + encoded.layout.num_rules;
        let bottom_vars: HashSet<Var> = (1..=num_bottom_vars).map(Var::new).collect();
        let bottom_solver = cdcl::Solver::new(&mut db_builder, &bottom_vars, bottom_external);

        // Top solver uses bottom's assigned literals as external input
        let top_external = bottom_solver.assigned_saved().get();

        // Variables for top solver: all variables
        let top_vars: HashSet<Var> = (1..=encoded.layout.total_vars()).map(Var::new).collect();
        let top_solver = cdcl::Solver::new(&mut db_builder, &top_vars, top_external);

        let mut db = db_builder.build();
        let mut bottom_solver = bottom_solver;
        let mut top_solver = top_solver;

        // Add bottom clauses
        for (i, clause) in encoded.bottom_clauses.iter().enumerate() {
            bottom_solver.add_clause(&mut db, i as u32, clause);
        }
        let mut next_id = encoded.bottom_clauses.len() as u32;

        // Add bottom PB constraints
        for (i, (terms, bound)) in encoded.bottom_pb_constraints.iter().enumerate() {
            bottom_solver.add_pb_constraint(&mut db, next_id + i as u32, terms, *bound);
        }
        next_id += encoded.bottom_pb_constraints.len() as u32;

        // Add top clauses
        for (i, clause) in encoded.top_clauses.iter().enumerate() {
            top_solver.add_clause(&mut db, i as u32, clause);
        }

        // Add top PB constraints
        let top_pb_id_offset = encoded.top_clauses.len() as u32;
        for (i, (terms, bound)) in encoded.top_pb_constraints.iter().enumerate() {
            top_solver.add_pb_constraint(&mut db, top_pb_id_offset + i as u32, terms, *bound);
        }

        AspSolver {
            program,
            encoded,
            db,
            bottom_solver,
            top_solver,
            next_bottom_clause_id: next_id,
        }
    }

    /// Find all stable models of the program.
    pub fn solve(&mut self) -> Vec<AnswerSet> {
        let start = Instant::now();
        let mut answer_sets = Vec::new();
        let mut bottom_calls = 0u64;
        let mut top_calls = 0u64;
        let mut loop_constraints = 0u64;

        loop {
            // Step 1: Solve bottom to find a candidate (keep the decision stack)
            let (sat, _stats) = self.bottom_solver.solve_and_stay(&mut self.db);
            bottom_calls += 1;
            if !sat {
                break; // No more candidates - UNSAT
            }

            // Step 2: Check if top solver finds a strictly smaller model
            // (bottom's assignments are visible to top via external relation)
            match self.check_minimality(&mut top_calls) {
                MinimalityResult::IsMinimal => {
                    // Found a stable model!
                    let answer_set = self.extract_answer_set();
                    answer_sets.push(answer_set);

                    // Add blocking clause to prevent finding same model again
                    let blocking = self.blocking_clause();
                    self.add_bottom_clause(&blocking);

                    // Backtrack to try again
                    self.bottom_solver.backtrack(&mut self.db, Level::TOP);
                }
                MinimalityResult::SmallerExists { difference } => {
                    // Not stable - add loop constraint
                    let loop_clause =
                        generate_loop_constraint(&difference, &self.program, &self.encoded.layout);

                    if loop_clause.is_empty() {
                        // No supporting rules - block this candidate
                        let blocking = self.blocking_clause();
                        self.add_bottom_clause(&blocking);
                        self.bottom_solver.backtrack(&mut self.db, Level::TOP);
                    } else {
                        // Add loop constraint and backtrack to asserting level
                        loop_constraints += 1;
                        let backtrack_level = self.compute_backtrack_level(&loop_clause);
                        self.add_bottom_clause(&loop_clause);
                        self.bottom_solver.backtrack(&mut self.db, backtrack_level);
                    }
                }
            }
        }

        eprintln!(
            "c asp: {:.3}s models={} bottom_calls={} top_calls={} loop_constraints={}",
            start.elapsed().as_secs_f64(),
            answer_sets.len(),
            bottom_calls,
            top_calls,
            loop_constraints
        );
        answer_sets
    }

    /// Add a clause to the bottom solver.
    fn add_bottom_clause(&mut self, clause: &Clause) {
        self.bottom_solver
            .add_clause(&mut self.db, self.next_bottom_clause_id, clause);
        self.next_bottom_clause_id += 1;
    }

    /// Compute the backtrack level for a learned clause.
    /// Returns the second-highest decision level among the clause literals.
    fn compute_backtrack_level(&self, clause: &Clause) -> Level {
        // Get all levels for literals in the clause
        // The clause contains literals that should become true, so we check the negation
        // Note: some literals may be unassigned (e.g., supporting rule variables)
        let mut levels: Vec<Level> = clause
            .iter()
            .filter_map(|&lit| self.bottom_solver.get_level(!lit))
            .collect();

        // Sort descending to find second-highest
        levels.sort_by(|a, b| b.cmp(a));
        levels.dedup();

        // Return second-highest level, or TOP if only one level
        levels.get(1).copied().unwrap_or(Level::TOP)
    }

    /// Check if the current bottom assignment is minimal.
    fn check_minimality(&mut self, top_calls: &mut u64) -> MinimalityResult {
        // Top solver sees bottom's assignments via external relation
        // Try to find a strictly smaller model
        let (sat, _stats) = self.top_solver.solve_and_stay(&mut self.db);
        *top_calls += 1;
        if sat {
            // Found a smaller model - extract the difference
            let bottom_assignment = self.bottom_solver.get_assignment();
            let top_assignment = self.top_solver.get_assignment();
            let difference = self.extract_difference(&bottom_assignment, &top_assignment);

            // Backtrack top solver
            self.top_solver.backtrack(&mut self.db, Level::TOP);

            MinimalityResult::SmallerExists { difference }
        } else {
            // No smaller model - candidate is minimal
            MinimalityResult::IsMinimal
        }
    }

    /// Extract atoms that are in bottom but not in top (the difference).
    fn extract_difference(
        &self,
        bottom: &contiguous_data::HashMap<Var, bool>,
        top_assignment: &contiguous_data::HashMap<Var, bool>,
    ) -> Vec<Atom> {
        let layout = &self.encoded.layout;
        let mut difference = Vec::new();

        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let bottom_var = layout.bottom(atom);
            let top_var = layout.top(atom);

            // Check if atom is true in bottom but false in top
            let in_bottom = bottom.get(&bottom_var).copied().unwrap_or(false);
            let in_top = top_assignment.get(&top_var).copied().unwrap_or(false);

            if in_bottom && !in_top {
                difference.push(atom);
            }
        }

        difference
    }

    /// Create a blocking clause to prevent finding the same bottom model again.
    fn blocking_clause(&self) -> Clause {
        let layout = &self.encoded.layout;
        let assignment = self.bottom_solver.get_assignment();
        let mut clause = Vec::new();

        // Include every atom: negate true atoms, keep false atoms positive
        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let var = layout.bottom(atom);
            if let Some(&value) = assignment.get(&var) {
                if value {
                    clause.push(Lit::neg(var));
                } else {
                    clause.push(Lit::pos(var));
                }
            }
        }

        clause
    }

    /// Extract the answer set from the current bottom assignment.
    fn extract_answer_set(&self) -> AnswerSet {
        let layout = &self.encoded.layout;
        let assignment = self.bottom_solver.get_assignment();
        let mut answer_set = HashSet::default();

        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let var = layout.bottom(atom);
            if let Some(&value) = assignment.get(&var)
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
        self.program.symbols.iter().any(|(a, _)| *a == atom)
    }

    /// Get atom name from symbol table.
    pub fn atom_name(&self, atom: Atom) -> Option<&str> {
        self.program
            .symbols
            .iter()
            .find(|(a, _)| *a == atom)
            .map(|(_, name)| name.as_str())
    }
}

/// Result of the minimality check.
enum MinimalityResult {
    /// The candidate is minimal (no strictly smaller model exists).
    IsMinimal,
    /// A strictly smaller model exists.
    SmallerExists {
        /// Atoms that are in bottom but not in top.
        difference: Vec<Atom>,
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
}
