//! SAT encoding for ASP programs with two-solver architecture.
//!
//! Variable layout:
//! - Atoms 1..N → x_bottom (var 1..N)
//! - Rules 0..R-1 → active_r_bottom (var N+1..N+R)
//! - Rules 0..R-1 → active_r_top (var N+R+1..N+2R)
//! - Atoms 1..N → x_top (var N+2R+1..N+2R+N)
//! - Atoms 1..N → x_diminished (var N+2R+N+1..N+2R+2N)

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

    /// Get the x_bottom variable for an atom.
    pub fn bottom(&self, atom: Atom) -> Var {
        Var::new(atom.0)
    }

    /// Get the active_r_bottom variable for a rule index.
    pub fn active_bottom(&self, rule_idx: u32) -> Var {
        Var::new(self.num_atoms + 1 + rule_idx)
    }

    /// Get the active_r_top variable for a rule index.
    pub fn active_top(&self, rule_idx: u32) -> Var {
        Var::new(self.num_atoms + self.num_rules + 1 + rule_idx)
    }

    /// Get the x_top variable for an atom.
    pub fn top(&self, atom: Atom) -> Var {
        Var::new(self.num_atoms + 2 * self.num_rules + atom.0)
    }

    /// Get the x_diminished variable for an atom.
    pub fn diminished(&self, atom: Atom) -> Var {
        Var::new(self.num_atoms + 2 * self.num_rules + self.num_atoms + atom.0)
    }

    /// Total number of variables.
    pub fn total_vars(&self) -> u32 {
        // bottom: N, active_bottom: R, active_top: R, top: N, diminished: N
        self.num_atoms + 2 * self.num_rules + self.num_atoms + self.num_atoms
    }
}

/// Encoded ASP program ready for solving.
pub struct EncodedProgram {
    pub layout: VarLayout,
    /// Clauses for the bottom solver only.
    pub bottom_clauses: Vec<Clause>,
    /// PB constraints for the bottom solver.
    pub bottom_pb_constraints: Vec<PBConstraint>,
    /// Clauses for the top solver (includes constraints on top/diminished vars).
    pub top_clauses: Vec<Clause>,
    /// PB constraints for the top solver.
    pub top_pb_constraints: Vec<PBConstraint>,
}

/// Encode an ASP program for the two-solver architecture.
pub fn encode_program(program: &Program) -> EncodedProgram {
    let layout = VarLayout::new(program);
    let mut bottom_clauses = Vec::new();
    let mut bottom_pb_constraints = Vec::new();
    let mut top_clauses = Vec::new();
    let mut top_pb_constraints = Vec::new();

    // Encode each rule
    for (rule_idx, rule) in program.rules.iter().enumerate() {
        let rule_idx = rule_idx as u32;
        match rule {
            Rule::Basic(r) => {
                encode_basic_rule(
                    r,
                    rule_idx,
                    &layout,
                    &mut bottom_clauses,
                    &mut bottom_pb_constraints,
                    &mut top_clauses,
                    &mut top_pb_constraints,
                );
            }
            Rule::Choice(r) => {
                encode_choice_rule(
                    r,
                    rule_idx,
                    &layout,
                    &mut bottom_clauses,
                    &mut bottom_pb_constraints,
                    &mut top_clauses,
                    &mut top_pb_constraints,
                );
            }
            Rule::Disjunctive(_) => {
                panic!("Disjunctive rules not yet supported");
            }
        }
    }

    // Add constraint that false atom (atom 1) is always false in bottom
    bottom_clauses.push(vec![Lit::neg(layout.bottom(Atom(1)))]);

    // Top solver constraints for each atom using PB constraint:
    // (¬a_top, 1) ∨ (a_bottom, 1) ∨ (¬a_diminished, 1) >= 2
    // This encodes: a_top → (a_bottom ∧ ¬a_diminished)
    for atom_id in 2..=layout.num_atoms {
        let atom = Atom(atom_id);
        let terms = vec![
            (Lit::neg(layout.top(atom)), 1),
            (Lit::pos(layout.bottom(atom)), 1),
            (Lit::neg(layout.diminished(atom)), 1),
        ];
        top_pb_constraints.push((terms, 2));
    }

    // At least one atom must be diminished (strict subset)
    // ∨ all a_diminished
    // Note: if there are no user atoms, this is an empty clause (FALSE),
    // which correctly makes top solver UNSAT (no smaller model than {})
    let mut diminished_clause = Vec::new();
    for atom_id in 2..=layout.num_atoms {
        diminished_clause.push(Lit::pos(layout.diminished(Atom(atom_id))));
    }
    top_clauses.push(diminished_clause);

    EncodedProgram {
        layout,
        bottom_clauses,
        bottom_pb_constraints,
        top_clauses,
        top_pb_constraints,
    }
}

/// Encode a basic/weight rule: h :- #sum{w1:b1; w2:b2; w3:not b3; ...} >= bound
///
/// Bottom PB constraint 1 (activation): body satisfied → rule active
/// (active_r, sum_weights - bound + 1) ∨ (¬b1, w1) ∨ (¬b2, w2) ∨ (b3, w3) ... >= (sum - bound + 1)
///
/// Bottom PB constraint 2 (reverse implication): rule active → body satisfied
/// (¬active_r, bound) ∨ (b1, w1) ∨ (b2, w2) ∨ (¬b3, w3) ... >= bound
///
/// Bottom clause: h ∨ ¬active_r
///
/// Top PB constraint: (¬active_r_bottom, bound) ∨ (active_r_top, bound) ∨ (¬b1_top, w1) ∨ ... >= bound
/// Top clause: ¬h_bottom ∨ h_top ∨ ¬active_r_top
fn encode_basic_rule(
    rule: &BasicRule,
    rule_idx: u32,
    layout: &VarLayout,
    bottom_clauses: &mut Vec<Clause>,
    bottom_pb_constraints: &mut Vec<PBConstraint>,
    top_clauses: &mut Vec<Clause>,
    top_pb_constraints: &mut Vec<PBConstraint>,
) {
    let active_bottom = layout.active_bottom(rule_idx);
    let active_top = layout.active_top(rule_idx);

    // Calculate sum of weights
    let sum_weights: Weight = rule.body.iter().map(|lit| lit.weight).sum();
    let bound = rule.bound;

    // Activation weight: sum - bound + 1
    // This ensures: if body is satisfied (sum >= bound), then rule must be active
    let activation_weight = sum_weights - bound + 1;

    // Bottom PB constraint 1 (activation): body satisfied → rule active
    // (active_r, activation_weight) ∨ (negated body literals with weights) >= activation_weight
    // For facts (empty body), this becomes (active_r, 1) >= 1, forcing the rule active
    let mut activation_terms = vec![(Lit::pos(active_bottom), activation_weight)];
    for lit in &rule.body {
        // Negate the literal for the activation constraint
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.bottom(lit.atom))
        } else {
            Lit::pos(layout.bottom(lit.atom))
        };
        activation_terms.push((cdcl_lit, lit.weight));
    }
    bottom_pb_constraints.push((activation_terms, activation_weight));

    // Bottom PB constraint 2 (reverse implication): rule active → body satisfied
    // (¬active_r, bound) ∨ (body literals with weights) >= bound
    // Only needed if bound > 0 (otherwise trivially satisfied)
    if bound > 0 {
        let mut reverse_terms = vec![(Lit::neg(active_bottom), bound)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                Lit::pos(layout.bottom(lit.atom))
            } else {
                Lit::neg(layout.bottom(lit.atom))
            };
            reverse_terms.push((cdcl_lit, lit.weight));
        }
        bottom_pb_constraints.push((reverse_terms, bound));
    }

    // Bottom clause: h_bottom ∨ ¬active_r_bottom
    // This says: if rule is active, head must be true
    if !rule.head.is_false() {
        bottom_clauses.push(vec![
            Lit::pos(layout.bottom(rule.head)),
            Lit::neg(active_bottom),
        ]);
    } else {
        // Constraint rule: if active, contradiction
        // ¬active_r_bottom (rule can never be active)
        bottom_clauses.push(vec![Lit::neg(active_bottom)]);
    }

    // Top PB constraint: same structure as bottom activation constraint
    // (¬active_r_bottom, activation_weight) ∨ (active_r_top, activation_weight) ∨ (negated body) >= activation_weight
    // This ensures: if bottom rule is active and body is satisfied in top, then top rule is active
    let mut reduct_terms = vec![
        (Lit::neg(active_bottom), activation_weight),
        (Lit::pos(active_top), activation_weight),
    ];
    for lit in &rule.body {
        // Same negation logic as bottom activation constraint
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.top(lit.atom))
        } else {
            Lit::pos(layout.top(lit.atom))
        };
        reduct_terms.push((cdcl_lit, lit.weight));
    }
    top_pb_constraints.push((reduct_terms, activation_weight));

    // Top clause 2: ¬h_bottom ∨ h_top ∨ ¬active_r_top
    // If head is true in bottom and top rule is active, head must be true in top
    if !rule.head.is_false() {
        top_clauses.push(vec![
            Lit::neg(layout.bottom(rule.head)),
            Lit::pos(layout.top(rule.head)),
            Lit::neg(active_top),
        ]);
    } else {
        // Constraint rule: if active_top, contradiction
        top_clauses.push(vec![Lit::neg(active_top)]);
    }
}

/// Encode a choice rule: {h1, h2, ...} :- #sum{w1:b1; w2:b2; ...} >= bound
///
/// Same as basic rule but WITHOUT head implication clause
/// (heads are optional in choice rules)
///
/// Top clauses include: ¬hi_bottom ∨ hi_top ∨ ¬active_r_top (for each head)
fn encode_choice_rule(
    rule: &ChoiceRule,
    rule_idx: u32,
    layout: &VarLayout,
    _bottom_clauses: &mut Vec<Clause>,
    bottom_pb_constraints: &mut Vec<PBConstraint>,
    top_clauses: &mut Vec<Clause>,
    top_pb_constraints: &mut Vec<PBConstraint>,
) {
    let active_bottom = layout.active_bottom(rule_idx);
    let active_top = layout.active_top(rule_idx);

    // Calculate sum of weights
    let sum_weights: Weight = rule.body.iter().map(|lit| lit.weight).sum();
    let bound = rule.bound;

    // Activation weight: sum - bound + 1
    let activation_weight = sum_weights - bound + 1;

    // Bottom PB constraint 1 (activation): body satisfied → rule active
    // For choice rules with empty body, this forces the rule active
    let mut activation_terms = vec![(Lit::pos(active_bottom), activation_weight)];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.bottom(lit.atom))
        } else {
            Lit::pos(layout.bottom(lit.atom))
        };
        activation_terms.push((cdcl_lit, lit.weight));
    }
    bottom_pb_constraints.push((activation_terms, activation_weight));

    // Bottom PB constraint 2 (reverse implication): rule active → body satisfied
    // Only needed if bound > 0
    if bound > 0 {
        let mut reverse_terms = vec![(Lit::neg(active_bottom), bound)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                Lit::pos(layout.bottom(lit.atom))
            } else {
                Lit::neg(layout.bottom(lit.atom))
            };
            reverse_terms.push((cdcl_lit, lit.weight));
        }
        bottom_pb_constraints.push((reverse_terms, bound));
    }

    // NOTE: No h_bottom ∨ ¬active_r_bottom clause - heads are OPTIONAL in choice rules

    // Top PB constraint: same structure as bottom activation constraint
    let mut reduct_terms = vec![
        (Lit::neg(active_bottom), activation_weight),
        (Lit::pos(active_top), activation_weight),
    ];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.top(lit.atom))
        } else {
            Lit::pos(layout.top(lit.atom))
        };
        reduct_terms.push((cdcl_lit, lit.weight));
    }
    top_pb_constraints.push((reduct_terms, activation_weight));

    // Top clause: ¬hi_bottom ∨ hi_top ∨ ¬active_r_top (for each head)
    for &head in &rule.heads {
        top_clauses.push(vec![
            Lit::neg(layout.bottom(head)),
            Lit::pos(layout.top(head)),
            Lit::neg(active_top),
        ]);
    }
}

/// Generate a loop constraint for the bottom solver.
///
/// Given the difference between bottom and top models (atoms in bottom but not top),
/// find all rules that could support these atoms and require at least one to be active.
///
/// Clause: ¬y_bottom ∨ ¬z_bottom ∨ r1_active ∨ r2_active ∨ ...
pub fn generate_loop_constraint(
    difference: &[Atom],
    program: &Program,
    layout: &VarLayout,
) -> Clause {
    // Find rules that have any of the difference atoms in their head
    // but do NOT have any of the difference atoms in their positive body
    let diff_set: std::collections::HashSet<Atom> = difference.iter().copied().collect();

    let mut supporting_rules = Vec::new();

    for (rule_idx, rule) in program.rules.iter().enumerate() {
        match rule {
            Rule::Basic(r) => {
                // Check if head is in difference
                if diff_set.contains(&r.head) {
                    // Check that no positive body atom is in difference
                    let body_in_diff = r
                        .body
                        .iter()
                        .any(|lit| lit.positive && diff_set.contains(&lit.atom));
                    if !body_in_diff {
                        supporting_rules.push(rule_idx as u32);
                    }
                }
            }
            Rule::Choice(r) => {
                // Check if any head is in difference
                let head_in_diff = r.heads.iter().any(|h| diff_set.contains(h));
                if head_in_diff {
                    // Check that no positive body atom is in difference
                    let body_in_diff = r
                        .body
                        .iter()
                        .any(|lit| lit.positive && diff_set.contains(&lit.atom));
                    if !body_in_diff {
                        supporting_rules.push(rule_idx as u32);
                    }
                }
            }
            Rule::Disjunctive(_) => {
                panic!("Disjunctive rules not yet supported");
            }
        }
    }

    // Build clause: ¬y_bottom ∨ ¬z_bottom ∨ r1_active ∨ r2_active ∨ ...
    let mut clause = Vec::new();

    // Negated difference atoms
    for &atom in difference {
        clause.push(Lit::neg(layout.bottom(atom)));
    }

    // Supporting rules must have at least one active
    for rule_idx in supporting_rules {
        clause.push(Lit::pos(layout.active_bottom(rule_idx)));
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

        // bottom: 1, 2, 3
        assert_eq!(layout.bottom(Atom(1)).raw(), 1);
        assert_eq!(layout.bottom(Atom(3)).raw(), 3);

        // active_bottom: 4, 5
        assert_eq!(layout.active_bottom(0).raw(), 4);
        assert_eq!(layout.active_bottom(1).raw(), 5);

        // active_top: 6, 7
        assert_eq!(layout.active_top(0).raw(), 6);
        assert_eq!(layout.active_top(1).raw(), 7);

        // top: 8, 9, 10
        assert_eq!(layout.top(Atom(1)).raw(), 8);
        assert_eq!(layout.top(Atom(3)).raw(), 10);

        // diminished: 11, 12, 13
        assert_eq!(layout.diminished(Atom(1)).raw(), 11);
        assert_eq!(layout.diminished(Atom(3)).raw(), 13);

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

        let mut bottom = Vec::new();
        let mut bottom_pb = Vec::new();
        let mut top = Vec::new();
        let mut top_pb = Vec::new();
        encode_basic_rule(&rule, 0, &layout, &mut bottom, &mut bottom_pb, &mut top, &mut top_pb);

        // Bottom should have 1 clause:
        // h_bottom ∨ ¬active_bottom_0
        assert_eq!(bottom.len(), 1);

        // Bottom should have 2 PB constraints (body size 2, bound 2):
        // 1. Activation: (active, 1) ∨ (¬b, 1) ∨ (c, 1) >= 1 (sum=2, bound=2, weight=1)
        // 2. Reverse: (¬active, 2) ∨ (b, 1) ∨ (¬c, 1) >= 2
        assert_eq!(bottom_pb.len(), 2);

        // Top should have 1 clause:
        // ¬h_bottom ∨ h_top ∨ ¬active_top_0
        assert_eq!(top.len(), 1);

        // Top should have 1 PB constraint:
        // (¬active_bottom, 1) ∨ (active_top, 1) ∨ (¬b_top, 1) ∨ (c_top, 1) >= 1
        assert_eq!(top_pb.len(), 1);
    }
}
