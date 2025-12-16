//! SAT encoding for ASP programs with two-solver architecture.
//!
//! Variable layout:
//! - Atoms 1..N → x_cand (var 1..N)
//! - Rules 0..R-1 → active_r_cand (var N+1..N+R)
//! - Rules 0..R-1 → active_r_check (var N+R+1..N+2R)
//! - Atoms 1..N → x_check (var N+2R+1..N+2R+N)
//! - Atoms 1..N → x_dim (var N+2R+N+1..N+2R+2N)

use cdcl::{Lit, Var, Weight};

use crate::types::{Atom, BasicRule, ChoiceRule, Program, Rule};

/// A clause is a disjunction of literals.
pub type Clause = Vec<Lit>;

/// A PB constraint: sum of (lit * weight) >= bound.
pub type PBConstraint = (Vec<(Lit, Weight)>, Weight);

/// Variable layout for the ASP encoding.
#[derive(Debug, Clone)]
pub struct VarLayout {
    /// Number of atoms (N)
    pub num_atoms: u32,
    /// Number of rules (R)
    pub num_rules: u32,
}

impl VarLayout {
    pub fn new(program: &Program) -> Self {
        let num_atoms = program.max_atom;
        let num_rules = program.rules.len() as u32;
        VarLayout {
            num_atoms,
            num_rules,
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

    /// Total number of variables.
    pub fn total_vars(&self) -> u32 {
        // cand: N, active_cand: R, active_check: R, check: N, dim: N
        self.num_atoms + 2 * self.num_rules + self.num_atoms + self.num_atoms
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
            Rule::Disjunctive(_) => {
                panic!("Disjunctive rules not yet supported");
            }
        }
    }

    // Add constraint that false atom (atom 1) is always false in candidate solver
    cand_clauses.push(vec![Lit::neg(layout.cand(Atom(1)))]);

    // Add single-atom loop constraints for each atom (Constraint 8 initialization)
    for atom_id in 2..=layout.num_atoms {
        let clause = generate_loop_constraint(&[Atom(atom_id)], program, &layout);
        cand_clauses.push(clause);
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
/// Candidate Constraint 3 (head propagation):
/// h_cand ∨ ¬active_r_cand
///
/// Check Constraint 6 (reduct body satisfaction):
/// W_r · ¬active_r_cand + W_r · active_r_check + Σ w_i · ¬b_i_check + Σ u_j · c_j_check >= W_r
///
/// Check Constraint 7 (reduct head propagation):
/// ¬h_cand ∨ h_check ∨ ¬active_r_check
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

    // Constraint 3 (head propagation): h_cand ∨ ¬active_r_cand
    if !rule.head.is_false() {
        cand_clauses.push(vec![
            Lit::pos(layout.cand(rule.head)),
            Lit::neg(active_cand),
        ]);
    } else {
        // Constraint rule: if active, contradiction → ¬active_r_cand
        cand_clauses.push(vec![Lit::neg(active_cand)]);
    }

    // Constraint 6 (reduct body satisfaction):
    // W_r · ¬active_r_cand + W_r · active_r_check + Σ w_i · ¬b_i_check + Σ u_j · c_j_check >= W_r
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

    // Constraint 7 (reduct head propagation): ¬h_cand ∨ h_check ∨ ¬active_r_check
    if !rule.head.is_false() {
        check_clauses.push(vec![
            Lit::neg(layout.cand(rule.head)),
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

    // Constraint 7 (reduct head propagation): ¬h_cand ∨ h_check ∨ ¬active_r_check (for each head)
    for &head in &rule.heads {
        check_clauses.push(vec![
            Lit::neg(layout.cand(head)),
            Lit::pos(layout.check(head)),
            Lit::neg(active_check),
        ]);
    }
}

/// Generate a loop constraint for the candidate solver (Constraint 8).
///
/// Given an unfounded set U (atoms in candidate but not in check model),
/// find external support rules and require at least one to be active.
///
/// Clause: Σ ¬x_cand (for x ∈ U) + Σ active_r_cand (for external r) >= 1
pub fn generate_loop_constraint(
    unfounded_set: &[Atom],
    program: &Program,
    layout: &VarLayout,
) -> Clause {
    // Find external support rules: rules r where heads(r) ∩ U ≠ ∅ but body⁺(r) ∩ U = ∅
    let u_set: std::collections::HashSet<Atom> = unfounded_set.iter().copied().collect();

    let mut external_rules = Vec::new();

    for (rule_idx, rule) in program.rules.iter().enumerate() {
        match rule {
            Rule::Basic(r) => {
                // Check if head is in unfounded set
                if u_set.contains(&r.head) {
                    // Check that no positive body atom is in unfounded set
                    let body_in_u = r
                        .body
                        .iter()
                        .any(|lit| lit.positive && u_set.contains(&lit.atom));
                    if !body_in_u {
                        external_rules.push(rule_idx as u32);
                    }
                }
            }
            Rule::Choice(r) => {
                // Check if any head is in unfounded set
                let head_in_u = r.heads.iter().any(|h| u_set.contains(h));
                if head_in_u {
                    // Check that no positive body atom is in unfounded set
                    let body_in_u = r
                        .body
                        .iter()
                        .any(|lit| lit.positive && u_set.contains(&lit.atom));
                    if !body_in_u {
                        external_rules.push(rule_idx as u32);
                    }
                }
            }
            Rule::Disjunctive(_) => {
                panic!("Disjunctive rules not yet supported");
            }
        }
    }

    // Build clause: Σ ¬x_cand + Σ active_r_cand >= 1
    let mut clause = Vec::new();

    // Negated unfounded atoms
    for &atom in unfounded_set {
        clause.push(Lit::neg(layout.cand(atom)));
    }

    // External rules must have at least one active
    for rule_idx in external_rules {
        clause.push(Lit::pos(layout.active_cand(rule_idx)));
    }

    clause
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WeightedLit;

    #[test]
    fn test_var_layout() {
        // 3 atoms, 2 rules
        let layout = VarLayout {
            num_atoms: 3,
            num_rules: 2,
        };

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

        assert_eq!(layout.total_vars(), 13);
    }

    #[test]
    fn test_encode_basic_rule() {
        // h :- b, not c  (atoms: h=2, b=3, c=4)
        // This is a basic rule with weights all 1 and bound = 2
        let rule = BasicRule {
            head: Atom(2),
            body: vec![
                WeightedLit::pos(Atom(3), 1),
                WeightedLit::neg(Atom(4), 1),
            ],
            bound: 2,
        };

        let layout = VarLayout {
            num_atoms: 4,
            num_rules: 1,
        };

        let mut cand = Vec::new();
        let mut cand_pb = Vec::new();
        let mut check = Vec::new();
        let mut check_pb = Vec::new();
        encode_basic_rule(&rule, 0, &layout, &mut cand, &mut cand_pb, &mut check, &mut check_pb);

        // Candidate solver should have 1 clause (Constraint 3):
        // h_cand ∨ ¬active_cand_0
        assert_eq!(cand.len(), 1);

        // Candidate solver should have 2 PB constraints (Constraints 1 and 2):
        // 1. Body satisfaction: (active, 1) + (¬b, 1) + (c, 1) >= 1
        // 2. Body falsification: (¬active, 2) + (b, 1) + (¬c, 1) >= 2
        assert_eq!(cand_pb.len(), 2);

        // Check solver should have 1 clause (Constraint 7):
        // ¬h_cand ∨ h_check ∨ ¬active_check_0
        assert_eq!(check.len(), 1);

        // Check solver should have 1 PB constraint (Constraint 6):
        // (¬active_cand, 1) + (active_check, 1) + (¬b_check, 1) + (c_check, 1) >= 1
        assert_eq!(check_pb.len(), 1);
    }
}
