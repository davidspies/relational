//! SAT encoding for ASP programs with two-solver architecture.
//!
//! Variable layout:
//! - Atoms 1..N → x_cand (var 1..N)
//! - Rules 0..R-1 → active_r_cand (var N+1..N+R)
//! - Rules 0..R-1 → active_r_check (var N+R+1..N+2R)
//! - Atoms 1..N → x_check (var N+2R+1..N+2R+N)
//! - Atoms 1..N → x_dim (var N+2R+N+1..N+2R+2N)
//! - Non-choice rules → used_r_cand (var N+2R+2N+1..N+2R+2N+NC)
//! - Non-choice rule heads → active_r_h_cand (var N+2R+2N+NC+1..N+2R+2N+NC+TH)
//!
//! Where NC = number of non-choice rules, TH = total heads across non-choice rules.

use std::collections::HashMap;

use cdcl::{Lit, Var, Weight};

use crate::types::{Atom, BasicRule, ChoiceRule, DisjunctiveRule, Program, Rule};

/// Entry in the atom→rules index for generate_loop_constraint.
#[derive(Debug, Clone, Copy)]
pub struct HeadEntry {
    pub rule_idx: u32,
    pub head_idx: u32,
    pub is_choice: bool,
}

/// A clause is a disjunction of literals.
pub type Clause = Vec<Lit>;

/// A PB constraint: sum of (lit * weight) >= bound.
pub type PBConstraint = (Vec<(Lit, Weight)>, Weight);

/// Per-rule info for non-choice rules (basic or disjunctive).
#[derive(Debug, Clone)]
struct NonChoiceRuleInfo {
    /// Index among non-choice rules (for used_cand offset)
    non_choice_idx: u32,
    /// Number of heads
    num_heads: u32,
    /// Starting offset for head variables
    head_var_start: u32,
}

/// Variable layout for the ASP encoding.
#[derive(Debug, Clone)]
pub struct VarLayout {
    /// Number of atoms (N)
    pub num_atoms: u32,
    /// Number of rules (R)
    pub num_rules: u32,
    /// Number of non-choice rules
    num_non_choice: u32,
    /// Total head variables across all non-choice rules
    total_head_vars: u32,
    /// Info for each rule: Some(info) for non-choice, None for choice
    rule_info: Vec<Option<NonChoiceRuleInfo>>,
    /// Index from atom → rules that have this atom as a head
    atom_to_rules: HashMap<Atom, Vec<HeadEntry>>,
}

impl VarLayout {
    pub fn new(program: &Program) -> Self {
        let num_atoms = program.max_atom;
        let num_rules = program.rules.len() as u32;

        let mut rule_info = Vec::with_capacity(program.rules.len());
        let mut atom_to_rules: HashMap<Atom, Vec<HeadEntry>> = HashMap::new();
        let mut non_choice_idx = 0u32;
        let mut head_var_offset = 0u32;

        for (rule_idx, rule) in program.rules.iter().enumerate() {
            let rule_idx = rule_idx as u32;
            match rule {
                Rule::Basic(r) => {
                    let num_heads = if r.head.is_false() { 0 } else { 1 };
                    rule_info.push(Some(NonChoiceRuleInfo {
                        non_choice_idx,
                        num_heads,
                        head_var_start: head_var_offset,
                    }));
                    if !r.head.is_false() {
                        atom_to_rules.entry(r.head).or_default().push(HeadEntry {
                            rule_idx,
                            head_idx: 0,
                            is_choice: false,
                        });
                    }
                    non_choice_idx += 1;
                    head_var_offset += num_heads;
                }
                Rule::Disjunctive(r) => {
                    let num_heads = r.heads.len() as u32;
                    rule_info.push(Some(NonChoiceRuleInfo {
                        non_choice_idx,
                        num_heads,
                        head_var_start: head_var_offset,
                    }));
                    for (head_idx, &head) in r.heads.iter().enumerate() {
                        atom_to_rules.entry(head).or_default().push(HeadEntry {
                            rule_idx,
                            head_idx: head_idx as u32,
                            is_choice: false,
                        });
                    }
                    non_choice_idx += 1;
                    head_var_offset += num_heads;
                }
                Rule::Choice(r) => {
                    rule_info.push(None);
                    for (head_idx, &head) in r.heads.iter().enumerate() {
                        atom_to_rules.entry(head).or_default().push(HeadEntry {
                            rule_idx,
                            head_idx: head_idx as u32,
                            is_choice: true,
                        });
                    }
                }
            }
        }

        VarLayout {
            num_atoms,
            num_rules,
            num_non_choice: non_choice_idx,
            total_head_vars: head_var_offset,
            rule_info,
            atom_to_rules,
        }
    }

    /// Get the x_cand variable for an atom.
    pub fn cand(&self, atom: Atom) -> Var {
        Var::new(atom.0)
    }

    /// Get the active_r_cand variable for a rule index.
    pub fn active_cand(&self, rule_idx: u32) -> Var {
        Var::new(self.num_atoms + 1 + rule_idx)
    }

    /// Get the active_r_check variable for a rule index.
    pub fn active_check(&self, rule_idx: u32) -> Var {
        Var::new(self.num_atoms + self.num_rules + 1 + rule_idx)
    }

    /// Get the x_check variable for an atom.
    pub fn check(&self, atom: Atom) -> Var {
        Var::new(self.num_atoms + 2 * self.num_rules + atom.0)
    }

    /// Get the x_dim variable for an atom.
    pub fn dim(&self, atom: Atom) -> Var {
        Var::new(self.num_atoms + 2 * self.num_rules + self.num_atoms + atom.0)
    }

    /// Base offset for used_cand variables
    fn used_cand_base(&self) -> u32 {
        self.num_atoms + 2 * self.num_rules + 2 * self.num_atoms + 1
    }

    /// Get the used_r_cand variable for a non-choice rule.
    /// Panics if rule_idx is a choice rule.
    pub fn used_cand(&self, rule_idx: u32) -> Var {
        let info = self.rule_info[rule_idx as usize]
            .as_ref()
            .expect("used_cand called on choice rule");
        Var::new(self.used_cand_base() + info.non_choice_idx)
    }

    /// Base offset for active_head_cand variables
    fn active_head_cand_base(&self) -> u32 {
        self.used_cand_base() + self.num_non_choice
    }

    /// Get the active_r_h_cand variable for a specific head of a non-choice rule.
    /// head_idx is 0-based index into the rule's heads.
    /// Panics if rule_idx is a choice rule.
    pub fn active_head_cand(&self, rule_idx: u32, head_idx: u32) -> Var {
        let info = self.rule_info[rule_idx as usize]
            .as_ref()
            .expect("active_head_cand called on choice rule");
        assert!(head_idx < info.num_heads);
        Var::new(self.active_head_cand_base() + info.head_var_start + head_idx)
    }

    /// Check if a rule is a non-choice rule (basic or disjunctive).
    pub fn is_non_choice(&self, rule_idx: u32) -> bool {
        self.rule_info[rule_idx as usize].is_some()
    }

    /// Get the number of heads for a non-choice rule.
    pub fn num_heads(&self, rule_idx: u32) -> u32 {
        self.rule_info[rule_idx as usize]
            .as_ref()
            .map(|info| info.num_heads)
            .unwrap_or(0)
    }

    /// Get rules that have the given atom as a head.
    pub fn rules_for_head(&self, atom: Atom) -> &[HeadEntry] {
        self.atom_to_rules.get(&atom).map_or(&[], |v| v.as_slice())
    }

    /// Total number of variables.
    pub fn total_vars(&self) -> u32 {
        // cand: N, active_cand: R, active_check: R, check: N, dim: N,
        // used_cand: NC, active_head_cand: TH
        self.num_atoms
            + 2 * self.num_rules
            + 2 * self.num_atoms
            + self.num_non_choice
            + self.total_head_vars
    }
}

/// Encoded ASP program ready for solving.
pub struct EncodedProgram {
    pub layout: VarLayout,
    /// Clauses for the candidate solver only.
    pub cand_clauses: Vec<Clause>,
    /// PB constraints for the candidate solver.
    pub cand_pb_constraints: Vec<PBConstraint>,
    /// Clauses for the check solver (includes constraints on check/dim vars).
    pub check_clauses: Vec<Clause>,
    /// PB constraints for the check solver.
    pub check_pb_constraints: Vec<PBConstraint>,
}

/// Encode an ASP program for the two-solver architecture.
pub fn encode_program(program: &Program) -> EncodedProgram {
    let layout = VarLayout::new(program);
    let mut cand_clauses = Vec::new();
    let mut cand_pb_constraints = Vec::new();
    let mut check_clauses = Vec::new();
    let mut check_pb_constraints = Vec::new();

    // Encode each rule
    for (rule_idx, rule) in program.rules.iter().enumerate() {
        let rule_idx = rule_idx as u32;
        match rule {
            Rule::Basic(r) => {
                encode_basic_rule(
                    r,
                    rule_idx,
                    &layout,
                    &mut cand_clauses,
                    &mut cand_pb_constraints,
                    &mut check_clauses,
                    &mut check_pb_constraints,
                );
            }
            Rule::Choice(r) => {
                encode_choice_rule(
                    r,
                    rule_idx,
                    &layout,
                    &mut cand_clauses,
                    &mut cand_pb_constraints,
                    &mut check_clauses,
                    &mut check_pb_constraints,
                );
            }
            Rule::Disjunctive(r) => {
                encode_disjunctive_rule(
                    r,
                    rule_idx,
                    &layout,
                    &mut cand_clauses,
                    &mut cand_pb_constraints,
                    &mut check_clauses,
                    &mut check_pb_constraints,
                );
            }
        }
    }

    // Add constraint that false atom (atom 1) is always false in candidate solver
    cand_clauses.push(vec![Lit::neg(layout.cand(Atom(1)))]);

    // Add single-atom loop constraints for each atom (Constraint 13 initialization)
    for atom_id in 2..=layout.num_atoms {
        let pb_constraint = generate_loop_constraint(&[Atom(atom_id)], program, &layout);
        cand_pb_constraints.push(pb_constraint);
    }

    // Check solver constraints for each atom using PB constraint (Constraint 4):
    // (¬x_check, 1) + (x_cand, 1) + (¬x_dim, 1) >= 2
    // This encodes: x_check → (x_cand ∧ ¬x_dim)
    for atom_id in 2..=layout.num_atoms {
        let atom = Atom(atom_id);
        let terms = vec![
            (Lit::neg(layout.check(atom)), 1),
            (Lit::pos(layout.cand(atom)), 1),
            (Lit::neg(layout.dim(atom)), 1),
        ];
        check_pb_constraints.push((terms, 2));
    }

    // Constraint 5: At least one atom must be diminished (strict subset)
    // ∨ all x_dim
    // Note: if there are no user atoms, this is an empty clause (FALSE),
    // which correctly makes check solver UNSAT (no smaller model than {})
    let mut dim_clause = Vec::new();
    for atom_id in 2..=layout.num_atoms {
        dim_clause.push(Lit::pos(layout.dim(Atom(atom_id))));
    }
    check_clauses.push(dim_clause);

    EncodedProgram {
        layout,
        cand_clauses,
        cand_pb_constraints,
        check_clauses,
        check_pb_constraints,
    }
}

/// Encode a basic/weight rule: h :- #sum{w1:b1; w2:b2; w3:not b3; ...} >= bound
///
/// Candidate Constraint 1 (body satisfaction when active):
/// W_r · active_r_cand + Σ w_i · ¬b_i_cand + Σ u_j · c_j_cand >= W_r
///
/// Candidate Constraint 2 (body falsification when inactive):
/// t · ¬active_r_cand + Σ w_i · b_i_cand + Σ u_j · ¬c_j_cand >= t
///
/// Candidate Constraint 3 (head requirement): h_cand ∨ ¬active_r_cand
///
/// Candidate Constraint 4 (used implies active): ¬used_r_cand ∨ active_r_cand
///
/// Candidate Constraint 5 (head selection): ¬active_r_h_cand + used_r_cand >= 1
///
/// Candidate Constraint 6 (exclusive head): trivially satisfied for single-head rules
///
/// Candidate Constraint 7 (head propagation): h_cand ∨ ¬active_r_h_cand
///
/// Check Constraint 10 (reduct body satisfaction):
/// W_r · ¬active_r_cand + W_r · active_r_check + Σ w_i · ¬b_i_check + Σ u_j · c_j_check >= W_r
///
/// Check Constraint 11 (reduct head implication): h_check ∨ ¬active_r_check
fn encode_basic_rule(
    rule: &BasicRule,
    rule_idx: u32,
    layout: &VarLayout,
    cand_clauses: &mut Vec<Clause>,
    cand_pb_constraints: &mut Vec<PBConstraint>,
    check_clauses: &mut Vec<Clause>,
    check_pb_constraints: &mut Vec<PBConstraint>,
) {
    let active_cand = layout.active_cand(rule_idx);
    let active_check = layout.active_check(rule_idx);

    // Calculate sum of weights
    let sum_weights: Weight = rule.body.iter().map(|lit| lit.weight).sum();
    let bound = rule.bound;

    // Falsification weight: W_r = sum - bound + 1
    // This ensures: if body is satisfied (sum >= bound), then rule must be active
    let falsification_weight = sum_weights - bound + 1;

    // Constraint 1 (body satisfaction when active):
    // W_r · active_r_cand + Σ w_i · ¬b_i_cand + Σ u_j · c_j_cand >= W_r
    // For facts (empty body), this becomes (active_r, 1) >= 1, forcing the rule active
    let mut constraint1_terms = vec![(Lit::pos(active_cand), falsification_weight)];
    for lit in &rule.body {
        // Negate positive body literals, keep negative body literals positive
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.cand(lit.atom))
        } else {
            Lit::pos(layout.cand(lit.atom))
        };
        constraint1_terms.push((cdcl_lit, lit.weight));
    }
    cand_pb_constraints.push((constraint1_terms, falsification_weight));

    // Constraint 2 (body falsification when inactive):
    // t · ¬active_r_cand + Σ w_i · b_i_cand + Σ u_j · ¬c_j_cand >= t
    // Only needed if bound > 0 (otherwise trivially satisfied)
    if bound > 0 {
        let mut constraint2_terms = vec![(Lit::neg(active_cand), bound)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                Lit::pos(layout.cand(lit.atom))
            } else {
                Lit::neg(layout.cand(lit.atom))
            };
            constraint2_terms.push((cdcl_lit, lit.weight));
        }
        cand_pb_constraints.push((constraint2_terms, bound));
    }

    // Constraint 3 (head requirement): h_cand ∨ ¬active_r_cand
    if !rule.head.is_false() {
        cand_clauses.push(vec![
            Lit::pos(layout.cand(rule.head)),
            Lit::neg(active_cand),
        ]);

        // Additional constraints for non-choice rules with a real head
        let used_cand = layout.used_cand(rule_idx);
        let active_head_cand = layout.active_head_cand(rule_idx, 0);

        // Constraint 4 (used implies active): ¬used_r_cand ∨ active_r_cand
        cand_clauses.push(vec![Lit::neg(used_cand), Lit::pos(active_cand)]);

        // Constraint 5 (head selection): ¬active_r_h_cand + used_r_cand >= 1
        // For single head, this is a clause
        cand_clauses.push(vec![Lit::neg(active_head_cand), Lit::pos(used_cand)]);

        // Constraint 6 (exclusive head): trivially satisfied when n=1

        // Constraint 7 (head propagation): h_cand ∨ ¬active_r_h_cand
        cand_clauses.push(vec![
            Lit::pos(layout.cand(rule.head)),
            Lit::neg(active_head_cand),
        ]);
    } else {
        // Constraint rule: if active, contradiction → ¬active_r_cand
        cand_clauses.push(vec![Lit::neg(active_cand)]);
    }

    // Constraint 10 (reduct body satisfaction):
    // W_r · ¬active_r_cand + W_r · active_r_check + Σ w_i · ¬b_i_check + Σ u_j · c_j_check >= W_r
    let mut constraint10_terms = vec![
        (Lit::neg(active_cand), falsification_weight),
        (Lit::pos(active_check), falsification_weight),
    ];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.check(lit.atom))
        } else {
            Lit::pos(layout.check(lit.atom))
        };
        constraint10_terms.push((cdcl_lit, lit.weight));
    }
    check_pb_constraints.push((constraint10_terms, falsification_weight));

    // Constraint 11 (reduct head implication): h_check ∨ ¬active_r_check
    if !rule.head.is_false() {
        check_clauses.push(vec![
            Lit::pos(layout.check(rule.head)),
            Lit::neg(active_check),
        ]);
    } else {
        // Constraint rule: ¬active_r_check
        check_clauses.push(vec![Lit::neg(active_check)]);
    }
}

/// Encode a choice rule: {h1, h2, ...} :- #sum{w1:b1; w2:b2; ...} >= bound
///
/// Same as basic rule but WITHOUT Constraint 3 (head propagation).
/// Heads are optional in choice rules.
///
/// Constraint 7 generates one clause per head: ¬h_cand ∨ h_check ∨ ¬active_r_check
fn encode_choice_rule(
    rule: &ChoiceRule,
    rule_idx: u32,
    layout: &VarLayout,
    _cand_clauses: &mut Vec<Clause>,
    cand_pb_constraints: &mut Vec<PBConstraint>,
    check_clauses: &mut Vec<Clause>,
    check_pb_constraints: &mut Vec<PBConstraint>,
) {
    let active_cand = layout.active_cand(rule_idx);
    let active_check = layout.active_check(rule_idx);

    // Calculate sum of weights
    let sum_weights: Weight = rule.body.iter().map(|lit| lit.weight).sum();
    let bound = rule.bound;

    // Falsification weight: W_r = sum - bound + 1
    let falsification_weight = sum_weights - bound + 1;

    // Constraint 1 (body satisfaction when active)
    // For choice rules with empty body, this forces the rule active
    let mut constraint1_terms = vec![(Lit::pos(active_cand), falsification_weight)];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.cand(lit.atom))
        } else {
            Lit::pos(layout.cand(lit.atom))
        };
        constraint1_terms.push((cdcl_lit, lit.weight));
    }
    cand_pb_constraints.push((constraint1_terms, falsification_weight));

    // Constraint 2 (body falsification when inactive)
    // Only needed if bound > 0
    if bound > 0 {
        let mut constraint2_terms = vec![(Lit::neg(active_cand), bound)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                Lit::pos(layout.cand(lit.atom))
            } else {
                Lit::neg(layout.cand(lit.atom))
            };
            constraint2_terms.push((cdcl_lit, lit.weight));
        }
        cand_pb_constraints.push((constraint2_terms, bound));
    }

    // NOTE: No Constraint 3 - heads are OPTIONAL in choice rules

    // Constraint 6 (reduct body satisfaction)
    let mut constraint6_terms = vec![
        (Lit::neg(active_cand), falsification_weight),
        (Lit::pos(active_check), falsification_weight),
    ];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.check(lit.atom))
        } else {
            Lit::pos(layout.check(lit.atom))
        };
        constraint6_terms.push((cdcl_lit, lit.weight));
    }
    check_pb_constraints.push((constraint6_terms, falsification_weight));

    // Constraint 12 (reduct head propagation): ¬h_cand ∨ h_check ∨ ¬active_r_check (for each head)
    for &head in &rule.heads {
        check_clauses.push(vec![
            Lit::neg(layout.cand(head)),
            Lit::pos(layout.check(head)),
            Lit::neg(active_check),
        ]);
    }
}

/// Encode a disjunctive rule: h1 | h2 | ... :- #sum{w1:b1; w2:b2; ...} >= bound
///
/// Candidate Constraint 1 (body satisfaction when active):
/// W_r · active_r_cand + Σ w_i · ¬b_i_cand + Σ u_j · c_j_cand >= W_r
///
/// Candidate Constraint 2 (body falsification when inactive):
/// t · ¬active_r_cand + Σ w_i · b_i_cand + Σ u_j · ¬c_j_cand >= t
///
/// Candidate Constraint 3 (head requirement): Σ h_cand + ¬active_r_cand >= 1
///
/// Candidate Constraint 4 (used implies active): ¬used_r_cand ∨ active_r_cand
///
/// Candidate Constraint 5 (head selection): Σ ¬active_r_h_cand + used_r_cand >= n
///
/// Candidate Constraint 6 (exclusive head): Σ ¬h_cand + (n-1)·¬used_r_cand >= n-1
///
/// Candidate Constraint 7 (head propagation): h_cand ∨ ¬active_r_h_cand (for each head)
///
/// Check Constraint 10 (reduct body satisfaction):
/// W_r · ¬active_r_cand + W_r · active_r_check + Σ w_i · ¬b_i_check + Σ u_j · c_j_check >= W_r
///
/// Check Constraint 11 (reduct head implication): Σ h_check + ¬active_r_check >= 1
fn encode_disjunctive_rule(
    rule: &DisjunctiveRule,
    rule_idx: u32,
    layout: &VarLayout,
    cand_clauses: &mut Vec<Clause>,
    cand_pb_constraints: &mut Vec<PBConstraint>,
    check_clauses: &mut Vec<Clause>,
    check_pb_constraints: &mut Vec<PBConstraint>,
) {
    let active_cand = layout.active_cand(rule_idx);
    let active_check = layout.active_check(rule_idx);
    let n = rule.heads.len() as Weight;

    // Calculate sum of weights
    let sum_weights: Weight = rule.body.iter().map(|lit| lit.weight).sum();
    let bound = rule.bound;

    // Falsification weight: W_r = sum - bound + 1
    let falsification_weight = sum_weights - bound + 1;

    // Constraint 1 (body satisfaction when active):
    // W_r · active_r_cand + Σ w_i · ¬b_i_cand + Σ u_j · c_j_cand >= W_r
    let mut constraint1_terms = vec![(Lit::pos(active_cand), falsification_weight)];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.cand(lit.atom))
        } else {
            Lit::pos(layout.cand(lit.atom))
        };
        constraint1_terms.push((cdcl_lit, lit.weight));
    }
    cand_pb_constraints.push((constraint1_terms, falsification_weight));

    // Constraint 2 (body falsification when inactive):
    // t · ¬active_r_cand + Σ w_i · b_i_cand + Σ u_j · ¬c_j_cand >= t
    if bound > 0 {
        let mut constraint2_terms = vec![(Lit::neg(active_cand), bound)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                Lit::pos(layout.cand(lit.atom))
            } else {
                Lit::neg(layout.cand(lit.atom))
            };
            constraint2_terms.push((cdcl_lit, lit.weight));
        }
        cand_pb_constraints.push((constraint2_terms, bound));
    }

    // Handle integrity constraints (no heads) vs disjunctive rules (with heads)
    if rule.heads.is_empty() {
        // Integrity constraint: if active, contradiction → ¬active_r_cand
        cand_clauses.push(vec![Lit::neg(active_cand)]);
        // Constraint 10: ¬active_r_check (if it were active, we'd need a head)
        check_clauses.push(vec![Lit::neg(active_check)]);
    } else {
        let used_cand = layout.used_cand(rule_idx);

        // Constraint 3 (head requirement): Σ h_cand + ¬active_r_cand >= 1
        // This is a clause: h1 ∨ h2 ∨ ... ∨ ¬active_r
        let mut constraint3_clause: Vec<Lit> = rule
            .heads
            .iter()
            .map(|&h| Lit::pos(layout.cand(h)))
            .collect();
        constraint3_clause.push(Lit::neg(active_cand));
        cand_clauses.push(constraint3_clause);

        // Constraint 4 (used implies active): ¬used_r_cand ∨ active_r_cand
        cand_clauses.push(vec![Lit::neg(used_cand), Lit::pos(active_cand)]);

        // Constraint 5 (head selection): Σ ¬active_r_h_cand + used_r_cand >= n
        let mut constraint5_terms: Vec<(Lit, Weight)> = (0..rule.heads.len())
            .map(|head_idx| {
                (
                    Lit::neg(layout.active_head_cand(rule_idx, head_idx as u32)),
                    1,
                )
            })
            .collect();
        constraint5_terms.push((Lit::pos(used_cand), n));
        cand_pb_constraints.push((constraint5_terms, n));

        // Constraint 6 (exclusive head): Σ ¬h_cand + (n-1)·¬used_r_cand >= n-1
        // Only needed if n > 1
        if n > 1 {
            let mut constraint6_terms: Vec<(Lit, Weight)> = rule
                .heads
                .iter()
                .map(|&h| (Lit::neg(layout.cand(h)), 1))
                .collect();
            constraint6_terms.push((Lit::neg(used_cand), n - 1));
            cand_pb_constraints.push((constraint6_terms, n - 1));
        }

        // Constraint 7 (head propagation): h_cand ∨ ¬active_r_h_cand (for each head)
        for (head_idx, &head) in rule.heads.iter().enumerate() {
            cand_clauses.push(vec![
                Lit::pos(layout.cand(head)),
                Lit::neg(layout.active_head_cand(rule_idx, head_idx as u32)),
            ]);
        }

        // Constraint 10 (reduct body satisfaction):
        // W_r · ¬active_r_cand + W_r · active_r_check + Σ w_i · ¬b_i_check + Σ u_j · c_j_check >= W_r
        let mut constraint10_terms = vec![
            (Lit::neg(active_cand), falsification_weight),
            (Lit::pos(active_check), falsification_weight),
        ];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                Lit::neg(layout.check(lit.atom))
            } else {
                Lit::pos(layout.check(lit.atom))
            };
            constraint10_terms.push((cdcl_lit, lit.weight));
        }
        check_pb_constraints.push((constraint10_terms, falsification_weight));

        // Constraint 11 (reduct head implication): Σ h_check + ¬active_r_check >= 1
        // This is a clause: h1_check ∨ h2_check ∨ ... ∨ ¬active_r_check
        let mut constraint11_clause: Vec<Lit> = rule
            .heads
            .iter()
            .map(|&h| Lit::pos(layout.check(h)))
            .collect();
        constraint11_clause.push(Lit::neg(active_check));
        check_clauses.push(constraint11_clause);
    }
}

/// Generate a loop constraint for the candidate solver (Constraint 13).
///
/// Given an unfounded set U (atoms in candidate but not in check model),
/// find external support and require at least one to be active.
///
/// Clause: Σ ¬x_cand (x ∈ U) + Σ active_r,h_cand (external (r,h)) >= 1
///
/// This is a simple disjunction: either some atom in U is false, or some
/// external support rule is active.
///
/// For choice rules, use active_r_cand directly.
/// For non-choice rules, use active_r,h_cand for each head h in U.
pub fn generate_loop_constraint(
    unfounded_set: &[Atom],
    program: &Program,
    layout: &VarLayout,
) -> PBConstraint {
    let u_set: std::collections::HashSet<Atom> = unfounded_set.iter().copied().collect();

    let mut terms: Vec<(Lit, Weight)> = Vec::new();

    // Negated unfounded atoms (each with weight 1)
    for &atom in unfounded_set {
        terms.push((Lit::neg(layout.cand(atom)), 1));
    }

    // Track which choice rules we've already added (they use active_r_cand, not per-head)
    let mut added_choice_rules: std::collections::HashSet<u32> = std::collections::HashSet::new();

    // Find external support using the atom→rules index
    for &atom in unfounded_set {
        for entry in layout.rules_for_head(atom) {
            let rule = &program.rules[entry.rule_idx as usize];
            let body = match rule {
                Rule::Basic(r) => &r.body,
                Rule::Choice(r) => &r.body,
                Rule::Disjunctive(r) => &r.body,
            };

            // Check if body depends on unfounded set
            let body_in_u = body
                .iter()
                .any(|lit| lit.positive && u_set.contains(&lit.atom));

            if body_in_u {
                continue;
            }

            if entry.is_choice {
                // Choice rules use active_r_cand (only add once per rule)
                if added_choice_rules.insert(entry.rule_idx) {
                    terms.push((Lit::pos(layout.active_cand(entry.rule_idx)), 1));
                }
            } else {
                // Non-choice rules use active_r,h_cand for this specific head
                terms.push((
                    Lit::pos(layout.active_head_cand(entry.rule_idx, entry.head_idx)),
                    1,
                ));
            }
        }
    }

    (terms, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WeightedLit;

    fn make_program(rules: Vec<Rule>, max_atom: u32) -> Program {
        Program {
            rules,
            symbols: Vec::new(),
            max_atom,
        }
    }

    #[test]
    fn test_var_layout() {
        // 3 atoms, 2 basic rules with single heads
        let program = make_program(
            vec![
                Rule::Basic(BasicRule {
                    head: Atom(2),
                    body: vec![],
                    bound: 0,
                }),
                Rule::Basic(BasicRule {
                    head: Atom(3),
                    body: vec![],
                    bound: 0,
                }),
            ],
            3,
        );
        let layout = VarLayout::new(&program);

        // cand: 1, 2, 3
        assert_eq!(layout.cand(Atom(1)).raw(), 1);
        assert_eq!(layout.cand(Atom(3)).raw(), 3);

        // active_cand: 4, 5
        assert_eq!(layout.active_cand(0).raw(), 4);
        assert_eq!(layout.active_cand(1).raw(), 5);

        // active_check: 6, 7
        assert_eq!(layout.active_check(0).raw(), 6);
        assert_eq!(layout.active_check(1).raw(), 7);

        // check: 8, 9, 10
        assert_eq!(layout.check(Atom(1)).raw(), 8);
        assert_eq!(layout.check(Atom(3)).raw(), 10);

        // dim: 11, 12, 13
        assert_eq!(layout.dim(Atom(1)).raw(), 11);
        assert_eq!(layout.dim(Atom(3)).raw(), 13);

        // used_cand for 2 non-choice rules: 14, 15
        assert_eq!(layout.used_cand(0).raw(), 14);
        assert_eq!(layout.used_cand(1).raw(), 15);

        // active_head_cand: each rule has 1 head, so: 16, 17
        assert_eq!(layout.active_head_cand(0, 0).raw(), 16);
        assert_eq!(layout.active_head_cand(1, 0).raw(), 17);

        // Total: 3 atoms + 2 active_cand + 2 active_check + 3 check + 3 dim
        //        + 2 used_cand + 2 active_head_cand = 17
        assert_eq!(layout.total_vars(), 17);
    }

    #[test]
    fn test_encode_basic_rule() {
        // h :- b, not c  (atoms: h=2, b=3, c=4)
        // This is a basic rule with weights all 1 and bound = 2
        let rule = BasicRule {
            head: Atom(2),
            body: vec![WeightedLit::pos(Atom(3), 1), WeightedLit::neg(Atom(4), 1)],
            bound: 2,
        };

        let program = make_program(vec![Rule::Basic(rule.clone())], 4);
        let layout = VarLayout::new(&program);

        let mut cand = Vec::new();
        let mut cand_pb = Vec::new();
        let mut check = Vec::new();
        let mut check_pb = Vec::new();
        encode_basic_rule(
            &rule,
            0,
            &layout,
            &mut cand,
            &mut cand_pb,
            &mut check,
            &mut check_pb,
        );

        // Candidate solver should have 4 clauses:
        // Constraint 3: h_cand ∨ ¬active_cand_0
        // Constraint 4: ¬used_cand ∨ active_cand
        // Constraint 5: ¬active_head_cand ∨ used_cand
        // Constraint 7: h_cand ∨ ¬active_head_cand
        assert_eq!(cand.len(), 4);

        // Candidate solver should have 2 PB constraints (Constraints 1 and 2)
        assert_eq!(cand_pb.len(), 2);

        // Check solver should have 1 clause (Constraint 11):
        // h_check ∨ ¬active_check_0
        assert_eq!(check.len(), 1);

        // Check solver should have 1 PB constraint (Constraint 10)
        assert_eq!(check_pb.len(), 1);
    }
}
