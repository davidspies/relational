//! Expansion of clasp solutions to full variable assignments.
//!
//! Given a set of true atoms from clasp, this module computes the semantically
//! correct values for all encoding variables (x_cand, active_r,s, used_r,s, etc.).

use std::collections::{HashMap, HashSet};

use cdcl::{Var, Weight};

use crate::encoding::{PBConstraint, VarLayout};
use crate::types::{Atom, Program, Rule, WeightedLit};

/// Expand a set of true atoms to a full variable assignment.
///
/// Given the atoms that clasp says are true, computes the values of all
/// encoding variables according to the expansion logic from the formalization.
pub fn expand_solution(
    program: &Program,
    layout: &VarLayout,
    true_atoms: &HashSet<Atom>,
) -> HashMap<Var, bool> {
    let mut assignment = HashMap::new();

    // 1. Atom variables (x_cand)
    // x_cand = true iff atom is in the answer set
    for atom_id in 1..=layout.num_atoms {
        let atom = Atom(atom_id);
        let var = layout.cand(atom);
        assignment.insert(var, true_atoms.contains(&atom));
    }

    // 2-4. Rule variables
    for (rule_idx, rule) in program.rules.iter().enumerate() {
        let rule_idx = rule_idx as u32;
        expand_rule(rule, rule_idx, layout, true_atoms, &mut assignment);
    }

    assignment
}

/// Expand a single rule's variables.
fn expand_rule(
    rule: &Rule,
    rule_idx: u32,
    layout: &VarLayout,
    true_atoms: &HashSet<Atom>,
    assignment: &mut HashMap<Var, bool>,
) {
    let (body, bound, heads, is_choice) = match rule {
        Rule::Choice(r) => (&r.body, r.bound, &r.heads, true),
        Rule::Disjunctive(r) => (&r.body, r.bound, &r.heads, false),
    };

    // Compute body weight sum
    let body_weight_sum = compute_body_weight(body, true_atoms);
    let _sum_weights = layout.sum_weights(rule_idx);
    let num_levels = layout.num_levels(rule_idx);

    // 2. active_r,s = true iff body_weight_sum >= s
    for level_offset in 0..num_levels {
        let level = bound + level_offset as Weight;
        let active_var = layout.active_cand(rule_idx, level);
        assignment.insert(active_var, body_weight_sum >= level);
    }

    // For choice rules, no used or active_head variables
    if is_choice {
        return;
    }

    // 3. active_r,h,s = true iff h is the ONLY true head AND body_weight >= s
    // Find which head (if any) is the only true one
    let only_true_head = find_only_true_head(heads, true_atoms);

    for level_offset in 0..num_levels {
        let level = bound + level_offset as Weight;
        let body_satisfied = body_weight_sum >= level;

        for (head_idx, _head) in heads.iter().enumerate() {
            let head_idx = head_idx as u32;
            let active_head_var = layout.active_head_cand(rule_idx, head_idx, level);

            let is_active = match only_true_head {
                Some(idx) if idx == head_idx => body_satisfied,
                _ => false,
            };
            assignment.insert(active_head_var, is_active);
        }

        // 4. used_r,s = true iff active_r,h,s for some head h
        let used_var = layout.used_cand(rule_idx, level);
        let is_used = only_true_head.is_some() && body_satisfied;
        assignment.insert(used_var, is_used);
    }
}

/// Compute the body weight sum given the true atoms.
fn compute_body_weight(body: &[WeightedLit], true_atoms: &HashSet<Atom>) -> Weight {
    body.iter()
        .map(|lit| {
            let atom_true = true_atoms.contains(&lit.atom);
            // Positive literal: add weight if atom is true
            // Negative literal: add weight if atom is false
            if lit.positive == atom_true {
                lit.weight
            } else {
                0
            }
        })
        .sum()
}

/// Find the only true head (returns Some(head_idx) if exactly one head is true).
fn find_only_true_head(heads: &[Atom], true_atoms: &HashSet<Atom>) -> Option<u32> {
    let mut only_true = None;
    for (idx, &head) in heads.iter().enumerate() {
        if true_atoms.contains(&head) {
            if only_true.is_some() {
                // More than one true head
                return None;
            }
            only_true = Some(idx as u32);
        }
    }
    only_true
}

/// Check a PB constraint against a variable assignment.
/// Returns (satisfied, lhs_value, bound).
pub fn check_constraint(
    constraint: &PBConstraint,
    assignment: &HashMap<Var, bool>,
) -> (bool, Weight, Weight) {
    let (terms, bound) = constraint;

    let lhs: Weight = terms
        .iter()
        .map(|(lit, weight)| {
            let var_value = assignment.get(&lit.var()).copied().unwrap_or(false);
            let lit_satisfied = if lit.is_positive() {
                var_value
            } else {
                !var_value
            };
            if lit_satisfied {
                *weight
            } else {
                0
            }
        })
        .sum();

    (lhs >= *bound, lhs, *bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdcl::Lit;
    use crate::types::{DisjunctiveRule, WeightedLit};

    fn make_program(rules: Vec<Rule>, max_atom: u32) -> Program {
        Program {
            rules,
            symbols: Vec::new(),
            max_atom,
        }
    }

    #[test]
    fn test_expand_basic_rule() {
        // Rule: h :- b.  (h=2, b=3)
        let program = make_program(
            vec![Rule::Disjunctive(DisjunctiveRule {
                heads: vec![Atom(2)],
                body: vec![WeightedLit::pos(Atom(3), 1)],
                bound: 1,
            })],
            3,
        );
        let layout = VarLayout::new(&program);

        // Model: {h, b} = {Atom(2), Atom(3)}
        let true_atoms: HashSet<Atom> = [Atom(2), Atom(3)].into_iter().collect();
        let assignment = expand_solution(&program, &layout, &true_atoms);

        // Check x_cand values
        assert!(!assignment[&layout.cand(Atom(1))]); // false atom
        assert!(assignment[&layout.cand(Atom(2))]); // h
        assert!(assignment[&layout.cand(Atom(3))]); // b

        // Check active (body is satisfied: b is true, weight 1 >= 1)
        assert!(assignment[&layout.active_cand_base(0)]);

        // Check used (h is only true head, body satisfied)
        assert!(assignment[&layout.used_cand_base(0)]);

        // Check active_head (h is active)
        assert!(assignment[&layout.active_head_cand_base(0, 0)]);
    }

    #[test]
    fn test_expand_multiple_true_heads() {
        // Rule: a | b :- c.  (a=2, b=3, c=4)
        let program = make_program(
            vec![Rule::Disjunctive(DisjunctiveRule {
                heads: vec![Atom(2), Atom(3)],
                body: vec![WeightedLit::pos(Atom(4), 1)],
                bound: 1,
            })],
            4,
        );
        let layout = VarLayout::new(&program);

        // Model: {a, b, c} - both heads true
        let true_atoms: HashSet<Atom> = [Atom(2), Atom(3), Atom(4)].into_iter().collect();
        let assignment = expand_solution(&program, &layout, &true_atoms);

        // active should be true (body satisfied)
        assert!(assignment[&layout.active_cand_base(0)]);

        // used should be false (multiple true heads)
        assert!(!assignment[&layout.used_cand_base(0)]);

        // active_head should be false for both (multiple true heads)
        assert!(!assignment[&layout.active_head_cand_base(0, 0)]);
        assert!(!assignment[&layout.active_head_cand_base(0, 1)]);
    }

    #[test]
    fn test_check_constraint() {
        let mut assignment = HashMap::new();
        let v1 = Var::new(1);
        let v2 = Var::new(2);
        assignment.insert(v1, true);
        assignment.insert(v2, false);

        // Constraint: 2·v1 + 3·¬v2 >= 4
        let constraint: PBConstraint = (
            vec![(Lit::pos(v1), 2), (Lit::neg(v2), 3)],
            4,
        );

        let (sat, lhs, bound) = check_constraint(&constraint, &assignment);
        assert!(sat);
        assert_eq!(lhs, 5); // 2 (v1=true) + 3 (v2=false)
        assert_eq!(bound, 4);
    }
}
