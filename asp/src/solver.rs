//! ASP solver using two CDCL SAT solvers.
//!
//! Architecture:
//! - Candidate solver: finds candidate answer sets
//! - Check solver: checks if a strictly smaller model exists (unfounded set detection)
//! - External relation: candidate's assignments feed into check as external input
//! - Both solvers share ONE database and retain learned clauses

use std::collections::HashMap;
use std::time::Instant;

use cdcl::{Level, Lit, Var};
use contiguous_data::HashSet;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use relational::create_persistent_input;
use relational::database::{Database, DatabaseBuilder};

use crate::encoding::{
    Clause, EncodedProgram, PBConstraint, VarKind, encode_program, generate_loop_constraint,
};
use crate::expansion::{check_constraint, expand_solution};
use crate::types::{Atom, Program, Rule};

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
    /// Seeded RNG for deterministic behavior
    rng: StdRng,
    /// Recorded UFS constraints for verification (only when ASP_VERIFY is set)
    recorded_ufs_constraints: Vec<RecordedConstraint>,
}

/// A recorded UFS constraint with context for debugging.
#[derive(Debug, Clone)]
pub struct RecordedConstraint {
    pub constraint: PBConstraint,
    pub chosen_atom: Atom,
    pub unfounded_set: Vec<Atom>,
    /// candidate_model[i] = true iff Atom(i+1) is true in the candidate model
    pub candidate_model: Vec<bool>,
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

        AspSolver {
            program,
            encoded,
            db,
            cand_solver,
            check_solver,
            next_cand_clause_id: next_id,
            atom_names,
            rng: StdRng::seed_from_u64(42),
            recorded_ufs_constraints: Vec::new(),
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
            // NECESSARY: limit=0 means unlimited, limit>0 means stop after that many
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
            // (candidate's assignments are visible to check via external relation)
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
                    // Pick a random atom from the unfounded set to assert
                    // (like clasp, we process one atom at a time)
                    let idx = self.rng.random_range(0..unfounded_set.len());
                    let chosen_atom = unfounded_set[idx];

                    // Not stable - add loop constraint
                    let (terms, bound) = generate_loop_constraint(
                        chosen_atom,
                        &unfounded_set,
                        &self.program,
                        &self.encoded.layout,
                    );

                    loop_constraints += 1;

                    // Record constraint for verification
                    self.recorded_ufs_constraints.push(RecordedConstraint {
                        constraint: (terms.clone(), bound),
                        chosen_atom,
                        unfounded_set: unfounded_set.clone(),
                        candidate_model: self.extract_candidate_model(),
                    });

                    // DEBUG: Conditional output for debugging loop constraint generation
                    if std::env::var("ASP_DEBUG").is_ok() && loop_constraints <= 3 {
                        eprintln!("c === Constraint #{} ===", loop_constraints);
                        eprintln!("c UFS: {:?}", unfounded_set);
                        eprintln!("c chosen={:?}", chosen_atom);
                        // Show rules for chosen atom
                        for entry in self.encoded.layout.rules_for_head(chosen_atom) {
                            let rule = &self.program.rules[entry.rule_idx as usize];
                            eprintln!("c   Rule {}: {:?}", entry.rule_idx, rule);
                        }
                        eprintln!("c Constraint terms: {:?}", terms);
                    }

                    // Dump candidate model if requested
                    if let Ok(target) = std::env::var("ASP_DUMP_CONSTRAINT") {
                        if target.parse::<u64>().ok() == Some(loop_constraints) {
                            eprintln!("c CONSTRAINT_TERMS: {:?}", terms);
                            eprintln!("c CONSTRAINT_SIZE: {}", terms.len());
                            // Decode each term
                            for (i, (lit, weight)) in terms.iter().enumerate() {
                                let var_kind = self.encoded.layout.decode_var(lit.var().raw());
                                let sign = if lit.is_positive() { "" } else { "¬" };
                                eprintln!(
                                    "c   Term {}: {}Var({}) = {:?}, weight={}",
                                    i + 1,
                                    sign,
                                    lit.var().raw(),
                                    var_kind,
                                    weight
                                );
                            }
                            // Also show the UFS
                            eprintln!("c UFS: {:?}", unfounded_set);
                            // Show rules for each UFS atom
                            for &ufs_atom in &unfounded_set {
                                for entry in self.encoded.layout.rules_for_head(ufs_atom) {
                                    let rule = &self.program.rules[entry.rule_idx as usize];
                                    eprintln!(
                                        "c   {:?} <- Rule {}: {:?}",
                                        ufs_atom, entry.rule_idx, rule
                                    );
                                }
                            }
                            let model = self.extract_candidate_model();
                            let bits: String = model
                                .iter()
                                .map(|&b| if b { '1' } else { '0' })
                                .collect();
                            eprintln!("c CANDIDATE_MODEL: {}", bits);
                        }
                    }

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
    /// Returns the second-highest decision level among the constraint literals.
    fn compute_backtrack_level_pb(&self, terms: &[(Lit, cdcl::Weight)]) -> Level {
        // Get all levels for literals in the constraint
        // The constraint contains literals that should become true, so we check the negation
        // Note: some literals may be unassigned (e.g., supporting rule variables)
        let mut levels: Vec<Level> = terms
            .iter()
            .filter_map(|&(lit, _)| self.cand_solver.get_level(!lit))
            .collect();

        // Sort descending to find second-highest
        levels.sort_by(|a, b| b.cmp(a));
        levels.dedup();

        // Return second-highest level, or TOP if only one level
        levels.get(1).copied().unwrap_or(Level::TOP)
    }

    /// Check if the current candidate assignment is minimal (no unfounded set exists).
    fn check_minimality(&mut self, check_calls: &mut u64) -> MinimalityResult {
        // Check solver sees candidate's assignments via external relation
        // Try to find a strictly smaller model (an unfounded set)
        let (sat, _stats) = self.check_solver.solve_and_stay(&mut self.db);
        *check_calls += 1;
        if sat {
            // Found a smaller model - extract the unfounded set
            let cand_assignment = self.cand_solver.get_assignment();
            let check_assignment = self.check_solver.get_assignment();
            let unfounded_set = self.extract_unfounded_set(&cand_assignment, &check_assignment);

            // Backtrack check solver
            self.check_solver.backtrack(&mut self.db, Level::TOP);

            MinimalityResult::SmallerExists { unfounded_set }
        } else {
            // No smaller model - candidate is minimal
            MinimalityResult::IsMinimal
        }
    }

    /// Extract unfounded set: atoms in candidate but not in check (S_cand \ S_check).
    fn extract_unfounded_set(
        &self,
        cand_assignment: &contiguous_data::HashMap<Var, bool>,
        check_assignment: &contiguous_data::HashMap<Var, bool>,
    ) -> Vec<Atom> {
        let layout = &self.encoded.layout;
        let mut unfounded = Vec::new();

        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let cand_var = layout.cand(atom);
            let check_var = layout.check(atom);

            // Check if atom is true in candidate but false in check
            let in_cand = cand_assignment.get(&cand_var).copied().unwrap_or(false);
            let in_check = check_assignment.get(&check_var).copied().unwrap_or(false);

            if in_cand && !in_check {
                unfounded.push(atom);
            }
        }

        unfounded
    }

    /// Create a blocking clause to prevent finding the same candidate model again.
    fn blocking_clause(&self) -> Clause {
        let layout = &self.encoded.layout;
        let assignment = self.cand_solver.get_assignment();
        let mut clause = Vec::new();

        // Blocking clause: negate current assignment
        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let var = layout.cand(atom);
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

    /// Extract the answer set from the current candidate assignment.
    fn extract_answer_set(&self) -> AnswerSet {
        let layout = &self.encoded.layout;
        let assignment = self.cand_solver.get_assignment();
        let mut answer_set = HashSet::default();

        // Extract true atoms with symbol names (non-auxiliary atoms)
        for atom_id in 2..=layout.num_atoms {
            let atom = Atom(atom_id);
            let var = layout.cand(atom);
            if let Some(&value) = assignment.get(&var)
                && value
                && self.is_shown_atom(atom)
            {
                answer_set.insert(atom);
            }
        }

        answer_set
    }

    /// Extract the full candidate model as Vec<bool>.
    /// result[i] = true iff Atom(i+1) is true in the current candidate assignment.
    fn extract_candidate_model(&self) -> Vec<bool> {
        let layout = &self.encoded.layout;
        let assignment = self.cand_solver.get_assignment();

        (1..=layout.num_atoms)
            .map(|atom_id| {
                let var = layout.cand(Atom(atom_id));
                assignment.get(&var).copied().unwrap_or(false)
            })
            .collect()
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

    /// Get the recorded UFS constraints for verification.
    pub fn recorded_constraints(&self) -> &[RecordedConstraint] {
        &self.recorded_ufs_constraints
    }

    /// Verify if a target solution (set of atom names) satisfies all recorded UFS constraints.
    /// Returns the first violated constraint, if any.
    ///
    /// Atom names can be:
    /// - Regular names from the symbol table (e.g., "comp(1,0,2)")
    /// - Synthetic names from name_all_atoms.py (e.g., "__atom_42")
    pub fn verify_solution(&self, target_atoms: &[&str]) -> Option<(usize, RecordedConstraint)> {
        // Build name→atom reverse lookup
        let name_to_atom: HashMap<&str, Atom> = self
            .atom_names
            .iter()
            .map(|(atom, name)| (name.as_str(), *atom))
            .collect();

        // Build the target atom set, handling both regular names and __atom_N synthetic names
        let target_atom_set: std::collections::HashSet<Atom> = target_atoms
            .iter()
            .filter_map(|name| {
                // Try regular symbol table lookup first
                if let Some(&atom) = name_to_atom.get(name) {
                    return Some(atom);
                }
                // Try parsing __atom_N format
                if name.starts_with("__atom_") {
                    if let Ok(id) = name[7..].parse::<u32>() {
                        return Some(Atom(id));
                    }
                }
                None
            })
            .collect();

        eprintln!("c Target model has {} atoms", target_atom_set.len());

        // Expand the solution to get full variable assignment
        let assignment = expand_solution(&self.program, &self.encoded.layout, &target_atom_set);
        eprintln!("c Expanded to {} variable assignments", assignment.len());

        // Count how many constraints have their chosen_atom in the target model
        let relevant_count = self
            .recorded_ufs_constraints
            .iter()
            .filter(|r| target_atom_set.contains(&r.chosen_atom))
            .count();
        eprintln!(
            "c {} of {} UFS constraints have chosen_atom in target model",
            relevant_count,
            self.recorded_ufs_constraints.len()
        );

        // Also check initial encoding constraints
        eprintln!(
            "c Checking {} initial cand_pb_constraints...",
            self.encoded.cand_pb_constraints.len()
        );
        let mut violations = 0;
        for (idx, constraint) in self.encoded.cand_pb_constraints.iter().enumerate() {
            let (satisfied, lhs, bound) = check_constraint(constraint, &assignment);
            if !satisfied {
                violations += 1;
                eprintln!(
                    "c INITIAL CONSTRAINT {} VIOLATED: sum={} < bound={}",
                    idx, lhs, bound
                );
                // Print details for debugging
                let (terms, _) = constraint;
                for &(lit, weight) in terms {
                    let var_value = assignment.get(&lit.var()).copied().unwrap_or(false);
                    let lit_satisfied = if lit.is_positive() {
                        var_value
                    } else {
                        !var_value
                    };
                    eprintln!(
                        "c   {:?}: var={}, lit_sat={}, weight={}",
                        lit, var_value, lit_satisfied, weight
                    );
                }
            }
        }
        eprintln!("c {} initial constraint violations", violations);

        // Check each recorded UFS constraint
        for (idx, recorded) in self.recorded_ufs_constraints.iter().enumerate() {
            let (satisfied, lhs, bound) = check_constraint(&recorded.constraint, &assignment);

            if !satisfied {
                // Print detailed info about violation
                eprintln!("c === Detailed violation analysis ===");
                eprintln!(
                    "c UFS Constraint {} VIOLATED: sum={} < bound={}",
                    idx, lhs, bound
                );
                let (terms, _) = &recorded.constraint;
                for &(lit, weight) in terms {
                    let var_raw = lit.var().raw();
                    let var_value = assignment.get(&lit.var()).copied().unwrap_or(false);
                    let lit_satisfied = if lit.is_positive() {
                        var_value
                    } else {
                        !var_value
                    };
                    let var_kind = self.encoded.layout.decode_var(var_raw);
                    eprintln!(
                        "c   {:?}: {:?}, value={}, lit_sat={}, weight={}",
                        lit, var_kind, var_value, lit_satisfied, weight
                    );
                }
                // Show which rules have chosen_atom as head
                eprintln!(
                    "c Rules with chosen_atom {:?} as head:",
                    recorded.chosen_atom
                );
                for entry in self.encoded.layout.rules_for_head(recorded.chosen_atom) {
                    let rule = &self.program.rules[entry.rule_idx as usize];
                    eprintln!("c   Rule {}: {:?}", entry.rule_idx, rule);
                    // Check if body depends on UFS
                    let u_set: std::collections::HashSet<Atom> =
                        recorded.unfounded_set.iter().copied().collect();
                    match rule {
                        crate::types::Rule::Disjunctive(r) => {
                            let body_in_u = r
                                .body
                                .iter()
                                .any(|lit| lit.positive && u_set.contains(&lit.atom));
                            eprintln!("c     body_in_u: {}", body_in_u);
                            // Check if body is satisfied in target
                            let body_sat = self
                                .is_rule_body_satisfied(entry.rule_idx as usize, &target_atom_set);
                            eprintln!("c     body_satisfied_in_target: {}", body_sat);
                        }
                        crate::types::Rule::Choice(r) => {
                            let body_in_u = r
                                .body
                                .iter()
                                .any(|lit| lit.positive && u_set.contains(&lit.atom));
                            eprintln!("c     body_in_u: {}", body_in_u);
                            let body_sat = self
                                .is_rule_body_satisfied(entry.rule_idx as usize, &target_atom_set);
                            eprintln!("c     body_satisfied_in_target: {}", body_sat);
                        }
                    }
                }
                return Some((idx, recorded.clone()));
            }
        }

        None
    }

    /// Compute the full stable model by forward propagation from shown atoms.
    /// This implements the Tp operator repeatedly until fixpoint.
    fn compute_full_model(
        &self,
        shown_atoms: &std::collections::HashSet<Atom>,
    ) -> std::collections::HashSet<Atom> {
        use crate::types::Rule;

        let mut model = shown_atoms.clone();
        let mut changed = true;

        while changed {
            changed = false;
            for rule in &self.program.rules {
                match rule {
                    Rule::Disjunctive(r) => {
                        // Check if body is satisfied
                        let mut sum = 0i64;
                        for lit in &r.body {
                            let atom_in = model.contains(&lit.atom);
                            let satisfied = if lit.positive { atom_in } else { !atom_in };
                            if satisfied {
                                sum += lit.weight as i64;
                            }
                        }
                        if sum >= r.bound as i64 {
                            // Body satisfied - derive heads that are in shown_atoms
                            // (we only derive heads that we know should be true)
                            for &head in &r.heads {
                                if !model.contains(&head) {
                                    // For basic rules (1 head), derive unconditionally
                                    // For disjunctive, we can't know which head without more info
                                    if r.heads.len() == 1 {
                                        model.insert(head);
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                    Rule::Choice(_) => {
                        // Choice rules don't force derivation
                    }
                }
            }
        }

        model
    }

    /// Get the value of a variable for a target solution.
    /// This interprets the variable based on its position in the layout.
    fn get_var_value_for_target(
        &self,
        var_raw: u32,
        target_atoms: &std::collections::HashSet<Atom>,
    ) -> bool {
        let layout = &self.encoded.layout;

        match layout.decode_var(var_raw) {
            VarKind::Cand(atom) => target_atoms.contains(&atom),

            VarKind::ActiveCand { rule_idx, level } => {
                // active_r,s_cand is true if body weight sum >= level
                self.is_rule_body_satisfied_at_level(rule_idx as usize, level, target_atoms)
            }

            VarKind::ActiveCheck { rule_idx } => {
                // For verification, assume check active if cand active at base level
                self.is_rule_body_satisfied(rule_idx as usize, target_atoms)
            }

            VarKind::Check(atom) => target_atoms.contains(&atom),

            VarKind::Dim(atom) => {
                // dim is true if atom is in target (for verification purposes)
                target_atoms.contains(&atom)
            }

            VarKind::UsedCand { rule_idx, level } => {
                // used_r,s_cand is true if rule is used at this level
                self.is_non_choice_rule_used_at_level(rule_idx, level, target_atoms)
            }

            VarKind::ActiveHeadCand {
                rule_idx,
                head_idx,
                level,
            } => {
                // active_r,h,s_cand is true if this head is active at this level
                self.is_active_head_cand_at_level(rule_idx, head_idx, level, target_atoms)
            }

            VarKind::Unknown(_) => false,
        }
    }

    /// Check if a rule's body is satisfied by the target atoms.
    fn is_rule_body_satisfied(
        &self,
        rule_idx: usize,
        target_atoms: &std::collections::HashSet<Atom>,
    ) -> bool {
        let rule = &self.program.rules[rule_idx];
        let (body, bound) = match rule {
            Rule::Choice(r) => (&r.body, r.bound),
            Rule::Disjunctive(r) => (&r.body, r.bound),
        };

        let mut sum = 0i64;
        for lit in body {
            let atom_in = target_atoms.contains(&lit.atom);
            let satisfied = if lit.positive { atom_in } else { !atom_in };
            if satisfied {
                sum += lit.weight as i64;
            }
        }
        sum >= bound as i64
    }

    /// Check if a rule's body weight sum >= level.
    fn is_rule_body_satisfied_at_level(
        &self,
        rule_idx: usize,
        level: crate::types::Weight,
        target_atoms: &std::collections::HashSet<Atom>,
    ) -> bool {
        let rule = &self.program.rules[rule_idx];
        let body = match rule {
            Rule::Choice(r) => &r.body,
            Rule::Disjunctive(r) => &r.body,
        };

        let mut sum = 0i64;
        for lit in body {
            let atom_in = target_atoms.contains(&lit.atom);
            let satisfied = if lit.positive { atom_in } else { !atom_in };
            if satisfied {
                sum += lit.weight as i64;
            }
        }
        sum >= level as i64
    }

    /// Check if a non-choice rule is used at a specific level.
    fn is_non_choice_rule_used_at_level(
        &self,
        rule_idx: u32,
        level: crate::types::Weight,
        target_atoms: &std::collections::HashSet<Atom>,
    ) -> bool {
        let rule = &self.program.rules[rule_idx as usize];
        if let Rule::Disjunctive(r) = rule {
            // Rule is used if body satisfied at level and exactly one head is true
            if !self.is_rule_body_satisfied_at_level(rule_idx as usize, level, target_atoms) {
                return false;
            }
            let heads_true: Vec<_> = r
                .heads
                .iter()
                .filter(|h| target_atoms.contains(h))
                .collect();
            return heads_true.len() == 1;
        }
        false
    }

    /// Check if a specific head is actively derived at a level.
    fn is_active_head_cand_at_level(
        &self,
        rule_idx: u32,
        head_idx: u32,
        level: crate::types::Weight,
        target_atoms: &std::collections::HashSet<Atom>,
    ) -> bool {
        let rule = &self.program.rules[rule_idx as usize];
        if let Rule::Disjunctive(r) = rule {
            // Body must be satisfied at this level
            if !self.is_rule_body_satisfied_at_level(rule_idx as usize, level, target_atoms) {
                return false;
            }
            // This head must be true
            let head = r.heads[head_idx as usize];
            if !target_atoms.contains(&head) {
                return false;
            }
            // And it must be the only true head (used rule)
            let heads_true: Vec<_> = r
                .heads
                .iter()
                .filter(|h| target_atoms.contains(h))
                .collect();
            return heads_true.len() == 1;
        }
        false
    }

    /// Count non-choice rules.
    fn count_non_choice_rules(&self) -> u32 {
        use crate::types::Rule;
        self.program
            .rules
            .iter()
            .filter(|r| matches!(r, Rule::Disjunctive(_)))
            .count() as u32
    }

    /// Check if a non-choice rule is "used" (actively supports one of its heads).
    fn is_non_choice_rule_used(
        &self,
        nc_idx: usize,
        target_atoms: &std::collections::HashSet<Atom>,
    ) -> bool {
        use crate::types::Rule;
        // Find the nc_idx-th non-choice rule
        let mut count = 0usize;
        for rule in &self.program.rules {
            if let Rule::Disjunctive(r) = rule {
                if count == nc_idx {
                    // Rule is used if body is satisfied and exactly one head is true
                    if !self.is_rule_body_satisfied_disj(r, target_atoms) {
                        return false;
                    }
                    let heads_true: Vec<_> = r
                        .heads
                        .iter()
                        .filter(|h| target_atoms.contains(h))
                        .collect();
                    return heads_true.len() == 1;
                }
                count += 1;
            }
        }
        false
    }

    /// Check if a disjunctive rule's body is satisfied.
    fn is_rule_body_satisfied_disj(
        &self,
        rule: &crate::types::DisjunctiveRule,
        target_atoms: &std::collections::HashSet<Atom>,
    ) -> bool {
        let mut sum = 0i64;
        for lit in &rule.body {
            let atom_in = target_atoms.contains(&lit.atom);
            let satisfied = if lit.positive { atom_in } else { !atom_in };
            if satisfied {
                sum += lit.weight as i64;
            }
        }
        sum >= rule.bound as i64
    }

    /// Check if a specific head is actively derived by its rule.
    fn is_active_head_cand(
        &self,
        var_raw: u32,
        active_head_base: u32,
        target_atoms: &std::collections::HashSet<Atom>,
    ) -> bool {
        use crate::types::Rule;
        // Map var_raw back to (rule_idx, head_idx)
        let offset = var_raw - active_head_base;
        let mut cumulative = 0u32;
        for (_rule_idx, rule) in self.program.rules.iter().enumerate() {
            if let Rule::Disjunctive(r) = rule {
                let num_heads = r.heads.len() as u32;
                if offset < cumulative + num_heads {
                    let head_idx = (offset - cumulative) as usize;
                    let head = r.heads[head_idx];
                    // This head is actively derived if:
                    // 1. The head is true
                    // 2. The body is satisfied
                    // 3. This rule is "chosen" to derive this head (used and this is the selected head)
                    if !target_atoms.contains(&head) {
                        return false;
                    }
                    if !self.is_rule_body_satisfied_disj(r, target_atoms) {
                        return false;
                    }
                    // For simplicity, assume this head is actively derived if it's the only true head
                    let heads_true: Vec<_> = r
                        .heads
                        .iter()
                        .enumerate()
                        .filter(|(_, h)| target_atoms.contains(h))
                        .collect();
                    return heads_true.len() == 1 && heads_true[0].0 == head_idx;
                }
                cumulative += num_heads;
            }
        }
        false
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
    fn test_loop_constraint_should_be_added_not_blocked() {
        // This test verifies bug 2: we should add loop constraints, not block candidates.
        //
        // Program:
        //   a :- {b, c} >= 1.
        //   b :- a.
        //   {c}.
        //   :- c.       (c must be false)
        //
        // When the solver tries candidate {a, b} (with c=false due to constraint):
        // - Check solver finds {} as smaller model
        // - UFS = {a, b, aux4, aux6}
        // - Due to bug 1, we incorrectly find "no external support"
        // - Bug 2: we block the candidate instead of adding ¬chosen_atom
        //
        // Expected: loop_constraints > 0 (we should add constraints, not just block)
        // Actual: loop_constraints = 0 (we're blocking instead)
        //
        // This test documents the bug - it currently passes because we're testing
        // the CURRENT (buggy) behavior. After fixing bug 2, the loop_constraints
        // count should increase.

        let input = "3 1 2 0 0\n1 1 1 0 2\n1 3 1 0 4\n1 5 1 0 3\n2 6 2 0 1 2 5\n1 4 1 0 6\n0\n2 c\n3 a\n5 b\n0\n";
        let program = crate::parse_smodels(input).unwrap();
        let mut solver = super::AspSolver::new(program);

        let results = solver.solve();

        // The result is correct (empty set only)
        assert_eq!(results.len(), 1);
        assert!(results[0].is_empty());

        // BUG: We're blocking candidates instead of adding loop constraints.
        // The recorded_constraints should have entries, but due to bug 2, it's empty.
        // This assertion documents the bug - it will FAIL once bug 2 is fixed.
        let loop_constraint_count = solver.recorded_constraints().len();
        assert!(
            loop_constraint_count > 0,
            "BUG: No loop constraints recorded. We're blocking instead of adding constraints. Count: {}",
            loop_constraint_count
        );
    }
}
