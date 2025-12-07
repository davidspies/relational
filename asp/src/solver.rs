//! ASP solver using two CDCL SAT solvers.
//!
//! Architecture:
//! - Bottom solver: finds candidate models using bottom clauses
//! - Top solver: checks if a strictly smaller model exists using top clauses
//! - External relation: bottom's assignments feed into top as external input

use cdcl::{Lit, Var};
use contiguous_data::HashSet;
use relational::create_persistent_input;
use relational::database::DatabaseBuilder;

use crate::encoding::{Clause, EncodedProgram, encode_program, generate_loop_constraint};
use crate::types::{Atom, Program};

/// An answer set (stable model).
pub type AnswerSet = HashSet<Atom>;

/// ASP Solver using two-solver architecture.
pub struct AspSolver {
    program: Program,
    encoded: EncodedProgram,
}

impl AspSolver {
    /// Create a new ASP solver for the given program.
    pub fn new(program: Program) -> Self {
        let encoded = encode_program(&program);
        AspSolver { program, encoded }
    }

    /// Find all stable models of the program.
    pub fn solve(&self) -> Vec<AnswerSet> {
        let mut answer_sets = Vec::new();
        let mut loop_constraints: Vec<Clause> = Vec::new();

        loop {
            // Step 1: Solve bottom clauses to get a candidate
            let candidate = match self.solve_bottom(&loop_constraints) {
                Some(c) => c,
                None => break, // No more candidates
            };

            // Step 2: Check if top solver finds a strictly smaller model
            match self.check_minimality(&candidate) {
                MinimalityResult::IsMinimal => {
                    // Found a stable model!
                    let answer_set = self.extract_answer_set(&candidate);
                    answer_sets.push(answer_set);

                    // Add blocking clause to prevent finding same bottom model again
                    let blocking = self.blocking_clause(&candidate);
                    loop_constraints.push(blocking);
                }
                MinimalityResult::SmallerExists { difference } => {
                    // Not stable - add loop constraint and try again
                    let loop_clause =
                        generate_loop_constraint(&difference, &self.program, &self.encoded.layout);
                    if loop_clause.is_empty() {
                        // No supporting rules - this shouldn't happen with well-formed programs
                        // Block this candidate and continue
                        let blocking = self.blocking_clause(&candidate);
                        loop_constraints.push(blocking);
                    } else {
                        loop_constraints.push(loop_clause);
                    }
                }
            }
        }

        answer_sets
    }

    /// Solve the bottom program to find a candidate model.
    fn solve_bottom(
        &self,
        loop_constraints: &[Clause],
    ) -> Option<contiguous_data::HashMap<Var, bool>> {
        let mut db_builder = DatabaseBuilder::new();
        create_persistent_input!(db_builder, _external_inp, external, Lit);

        // Variables: bottom + active only (top and diminished are not in bottom solver)
        let num_bottom_vars = self.encoded.layout.num_atoms + self.encoded.layout.num_rules;
        let vars: HashSet<Var> = (1..=num_bottom_vars).map(Var::new).collect();
        let mut solver = cdcl::Solver::new(&mut db_builder, &vars, external);

        let mut db = db_builder.build();

        // Add bottom clauses
        for (i, clause) in self.encoded.bottom_clauses.iter().enumerate() {
            solver.add_clause(&mut db, i as u32, clause);
        }

        // Add loop constraints from previous iterations
        let base_id = self.encoded.bottom_clauses.len() as u32;
        for (i, clause) in loop_constraints.iter().enumerate() {
            solver.add_clause(&mut db, base_id + i as u32, clause);
        }

        solver.solve(&mut db)
    }

    /// Check if the candidate is minimal (no strictly smaller model exists).
    fn check_minimality(
        &self,
        candidate: &contiguous_data::HashMap<Var, bool>,
    ) -> MinimalityResult {
        let mut db_builder = DatabaseBuilder::new();

        // Create external input for the bottom assignments
        create_persistent_input!(db_builder, external_inp, external, Lit);

        // All variables: bottom + active + top + diminished
        let vars: HashSet<Var> = (1..=self.encoded.layout.total_vars())
            .map(Var::new)
            .collect();
        let mut solver = cdcl::Solver::new(&mut db_builder, &vars, external);

        let mut db = db_builder.build();

        // Feed bottom assignments as external input
        for (&var, &value) in candidate.iter() {
            let lit = if value { Lit::pos(var) } else { Lit::neg(var) };
            external_inp.insert(lit);
        }
        db.commit();

        // Add top clauses
        for (i, clause) in self.encoded.top_clauses.iter().enumerate() {
            solver.add_clause(&mut db, i as u32, clause);
        }

        // Try to find a smaller model
        match solver.solve(&mut db) {
            Some(assignment) => {
                // Found a smaller model - extract the difference
                let difference = self.extract_difference(candidate, &assignment);
                MinimalityResult::SmallerExists { difference }
            }
            None => {
                // No smaller model - candidate is minimal
                MinimalityResult::IsMinimal
            }
        }
    }

    /// Extract atoms that are in bottom but not in top (the difference).
    fn extract_difference(
        &self,
        _bottom: &contiguous_data::HashMap<Var, bool>,
        top_assignment: &contiguous_data::HashMap<Var, bool>,
    ) -> Vec<Atom> {
        let layout = &self.encoded.layout;
        let mut difference = Vec::new();

        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let diminished_var = layout.diminished(atom);

            // Check if this atom is diminished (true in bottom, false in top)
            if let Some(&is_diminished) = top_assignment.get(&diminished_var) {
                if is_diminished {
                    difference.push(atom);
                }
            }
        }

        difference
    }

    /// Create a blocking clause to prevent finding the same bottom model again.
    fn blocking_clause(&self, candidate: &contiguous_data::HashMap<Var, bool>) -> Clause {
        let layout = &self.encoded.layout;
        let mut clause = Vec::new();

        // For each true bottom atom, add its negation to the blocking clause
        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let var = layout.bottom(atom);
            if let Some(&value) = candidate.get(&var) {
                if value {
                    clause.push(Lit::neg(var));
                }
            }
        }

        clause
    }

    /// Extract the answer set from a candidate (atoms that are true in the model).
    fn extract_answer_set(&self, candidate: &contiguous_data::HashMap<Var, bool>) -> AnswerSet {
        let layout = &self.encoded.layout;
        let mut answer_set = HashSet::default();

        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let var = layout.bottom(atom);
            if let Some(&value) = candidate.get(&var) {
                if value && self.is_shown_atom(atom) {
                    answer_set.insert(atom);
                }
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
