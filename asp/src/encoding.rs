//! SAT encoding for ASP programs with two-solver architecture.
//!
//! Variable layout:
//! - Atoms 1..N → x_bottom (var 1..N)
//! - Rules 0..R-1 → active_r (var N+1..N+R)
//! - Atoms 1..N → x_top (var N+R+1..N+R+N)
//! - Atoms 1..N → x_diminished (var N+R+N+1..N+R+2N)

use cdcl::{Lit, Var};

use crate::types::{Atom, BasicRule, Program, Rule};

/// A clause is a disjunction of literals.
pub type Clause = Vec<Lit>;

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

    /// Get the active_r variable for a rule index.
    pub fn active(&self, rule_idx: u32) -> Var {
        Var::new(self.num_atoms + 1 + rule_idx)
    }

    /// Get the x_top variable for an atom.
    pub fn top(&self, atom: Atom) -> Var {
        Var::new(self.num_atoms + self.num_rules + atom.0)
    }

    /// Get the x_diminished variable for an atom.
    pub fn diminished(&self, atom: Atom) -> Var {
        Var::new(self.num_atoms + self.num_rules + self.num_atoms + atom.0)
    }

    /// Total number of variables.
    pub fn total_vars(&self) -> u32 {
        // bottom: N, active: R, top: N, diminished: N
        self.num_atoms + self.num_rules + self.num_atoms + self.num_atoms
    }
}

/// Encoded ASP program ready for solving.
pub struct EncodedProgram {
    pub layout: VarLayout,
    /// Clauses for the bottom solver only.
    pub bottom_clauses: Vec<Clause>,
    /// Clauses for the top solver (includes constraints on top/diminished vars).
    pub top_clauses: Vec<Clause>,
}

/// Encode an ASP program for the two-solver architecture.
pub fn encode_program(program: &Program) -> EncodedProgram {
    let layout = VarLayout::new(program);
    let mut bottom_clauses = Vec::new();
    let mut top_clauses = Vec::new();

    // Encode each rule
    for (rule_idx, rule) in program.rules.iter().enumerate() {
        let rule_idx = rule_idx as u32;
        match rule {
            Rule::Basic(r) => {
                encode_basic_rule(r, rule_idx, &layout, &mut bottom_clauses, &mut top_clauses);
            }
            Rule::Choice(_) => {
                panic!("Choice rules not yet supported");
            }
            Rule::Disjunctive(_) => {
                panic!("Disjunctive rules not yet supported");
            }
        }
    }

    // Add constraint that false atom (atom 1) is always false in bottom
    bottom_clauses.push(vec![Lit::neg(layout.bottom(Atom(1)))]);

    // Top solver constraints for each atom
    for atom_id in 2..=layout.num_atoms {
        let atom = Atom(atom_id);

        // ¬a_top ∨ a_bottom (top implies bottom - subset constraint)
        top_clauses.push(vec![
            Lit::neg(layout.top(atom)),
            Lit::pos(layout.bottom(atom)),
        ]);

        // ¬a_diminished ∨ ¬a_top (diminished means not in top)
        top_clauses.push(vec![
            Lit::neg(layout.diminished(atom)),
            Lit::neg(layout.top(atom)),
        ]);

        // ¬a_diminished ∨ a_bottom (diminished means in bottom)
        top_clauses.push(vec![
            Lit::neg(layout.diminished(atom)),
            Lit::pos(layout.bottom(atom)),
        ]);
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
        top_clauses,
    }
}

/// Encode a basic rule: h :- b1, b2, b3, not b4
///
/// Bottom clauses:
/// - active_r ∨ ¬b1_bottom ∨ ¬b2_bottom ∨ ¬b3_bottom ∨ b4_bottom
/// - h_bottom ∨ ¬active_r
///
/// Top clause (when active):
/// - ¬active_r ∨ h_top ∨ ¬b1_top ∨ ¬b2_top ∨ ¬b3_top
fn encode_basic_rule(
    rule: &BasicRule,
    rule_idx: u32,
    layout: &VarLayout,
    bottom_clauses: &mut Vec<Clause>,
    top_clauses: &mut Vec<Clause>,
) {
    let active = layout.active(rule_idx);

    // Bottom clause 1: active_r ∨ ¬b1_bottom ∨ ... ∨ b4_bottom (negated body)
    // This says: if body is satisfied, rule must be active
    let mut activation_clause = vec![Lit::pos(active)];
    for &atom in &rule.pos_body {
        activation_clause.push(Lit::neg(layout.bottom(atom)));
    }
    for &atom in &rule.neg_body {
        activation_clause.push(Lit::pos(layout.bottom(atom)));
    }
    bottom_clauses.push(activation_clause);

    // Bottom clauses 2..N: reverse implication - if active, body must be satisfied
    // ¬active_r ∨ bi_bottom for each positive body atom
    for &atom in &rule.pos_body {
        bottom_clauses.push(vec![Lit::neg(active), Lit::pos(layout.bottom(atom))]);
    }
    // ¬active_r ∨ ¬ci_bottom for each negative body atom
    for &atom in &rule.neg_body {
        bottom_clauses.push(vec![Lit::neg(active), Lit::neg(layout.bottom(atom))]);
    }

    // Bottom clause: h_bottom ∨ ¬active_r
    // This says: if rule is active, head must be true
    if !rule.head.is_false() {
        bottom_clauses.push(vec![Lit::pos(layout.bottom(rule.head)), Lit::neg(active)]);
    } else {
        // Constraint rule: if active, contradiction
        // ¬active_r (rule can never be active)
        bottom_clauses.push(vec![Lit::neg(active)]);
    }

    // Top clause: ¬active_r ∨ h_top ∨ ¬b1_top ∨ ¬b2_top ∨ ...
    // Only positive body atoms (this is the positive reduct)
    let mut reduct_clause = vec![Lit::neg(active)];
    if !rule.head.is_false() {
        reduct_clause.push(Lit::pos(layout.top(rule.head)));
    }
    for &atom in &rule.pos_body {
        reduct_clause.push(Lit::neg(layout.top(atom)));
    }
    // Note: negative body atoms are NOT included in the reduct
    top_clauses.push(reduct_clause);
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
                    let body_in_diff = r.pos_body.iter().any(|a| diff_set.contains(a));
                    if !body_in_diff {
                        supporting_rules.push(rule_idx as u32);
                    }
                }
            }
            Rule::Choice(_) => {
                panic!("Choice rules not yet supported");
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
        clause.push(Lit::pos(layout.active(rule_idx)));
    }

    clause
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // active: 4, 5
        assert_eq!(layout.active(0).raw(), 4);
        assert_eq!(layout.active(1).raw(), 5);

        // top: 6, 7, 8
        assert_eq!(layout.top(Atom(1)).raw(), 6);
        assert_eq!(layout.top(Atom(3)).raw(), 8);

        // diminished: 9, 10, 11
        assert_eq!(layout.diminished(Atom(1)).raw(), 9);
        assert_eq!(layout.diminished(Atom(3)).raw(), 11);

        assert_eq!(layout.total_vars(), 11);
    }

    #[test]
    fn test_encode_basic_rule() {
        // h :- b, not c  (atoms: h=2, b=3, c=4)
        let rule = BasicRule {
            head: Atom(2),
            pos_body: vec![Atom(3)],
            neg_body: vec![Atom(4)],
        };

        let layout = VarLayout {
            num_atoms: 4,
            num_rules: 1,
        };

        let mut bottom = Vec::new();
        let mut top = Vec::new();
        encode_basic_rule(&rule, 0, &layout, &mut bottom, &mut top);

        // Bottom should have 4 clauses:
        // 1. active_0 ∨ ¬b_bottom ∨ c_bottom  (5, -3, 4)
        // 2. ¬active_0 ∨ b_bottom  (-5, 3)
        // 3. ¬active_0 ∨ ¬c_bottom  (-5, -4)
        // 4. h_bottom ∨ ¬active_0  (2, -5)
        assert_eq!(bottom.len(), 4);

        // Top should have 1 clause:
        // ¬active_0 ∨ h_top ∨ ¬b_top  (-5, 7, -8)
        assert_eq!(top.len(), 1);
    }
}
