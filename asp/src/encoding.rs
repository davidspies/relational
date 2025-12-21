//! SAT encoding for ASP programs with two-solver architecture.
//!
//! Variable layout (simplified - one active variable per rule):
//! - Atoms 1..N → x_cand (var 1..N)
//! - Rules 0..R-1 → active_r_cand (one per rule)
//! - Rules 0..R-1 → active_r_check (one per rule)
//! - Atoms 1..N → x_check
//! - Atoms 1..N → x_dim

use std::collections::{HashMap, HashSet};

use cdcl::{Lit, Var, Weight};
use contiguous_data::Multiset;

use crate::types::{Atom, ChoiceRule, DisjunctiveRule, Program, Rule, WeightedLit};

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

/// Variable layout for the ASP encoding.
#[derive(Debug, Clone)]
pub struct VarLayout {
    /// Number of atoms (N)
    pub num_atoms: u32,
    /// Number of rules (R)
    pub num_rules: u32,
    /// Base offset for active_cand variables
    active_cand_base: u32,
    /// Base offset for active_check variables
    active_check_base: u32,
    /// Base offset for check variables
    check_base: u32,
    /// Base offset for dim variables
    dim_base: u32,
    /// Total number of variables
    total_vars: u32,
    /// Index from atom → rules that have this atom as a head
    atom_to_rules: HashMap<Atom, Vec<HeadEntry>>,
    /// Bound (threshold) for each rule
    rule_bounds: Vec<Weight>,
    /// Sum of weights for each rule
    rule_sum_weights: Vec<Weight>,
}

/// Helper to compute sum of weights for a body
fn sum_weights(body: &[WeightedLit]) -> Weight {
    body.iter().map(|lit| lit.weight).sum()
}

impl VarLayout {
    pub fn new(program: &Program) -> Self {
        let num_atoms = program.max_atom;
        let num_rules = program.rules.len() as u32;

        let mut atom_to_rules: HashMap<Atom, Vec<HeadEntry>> = HashMap::new();
        let mut rule_bounds = Vec::with_capacity(program.rules.len());
        let mut rule_sum_weights = Vec::with_capacity(program.rules.len());

        // Build atom_to_rules index and collect rule info
        for (rule_idx, rule) in program.rules.iter().enumerate() {
            let rule_idx_u32 = rule_idx as u32;
            let (bound, body, heads, is_choice) = match rule {
                Rule::Disjunctive(r) => (r.bound, &r.body, &r.heads, false),
                Rule::Choice(r) => (r.bound, &r.body, &r.heads, true),
            };

            let sw = sum_weights(body);
            rule_bounds.push(bound);
            rule_sum_weights.push(sw);

            for (head_idx, &head) in heads.iter().enumerate() {
                atom_to_rules.entry(head).or_default().push(HeadEntry {
                    rule_idx: rule_idx_u32,
                    head_idx: head_idx as u32,
                    is_choice,
                });
            }
        }

        // Layout: atoms (1..N), active_cand, active_check, check, dim
        let active_cand_base = num_atoms + 1;
        let active_check_base = active_cand_base + num_rules;
        let check_base = active_check_base + num_rules;
        let dim_base = check_base + num_atoms;
        let total_vars = dim_base + num_atoms - 1;

        VarLayout {
            num_atoms,
            num_rules,
            active_cand_base,
            active_check_base,
            check_base,
            dim_base,
            total_vars,
            atom_to_rules,
            rule_bounds,
            rule_sum_weights,
        }
    }

    /// Get the x_cand variable for an atom.
    pub fn cand(&self, atom: Atom) -> Var {
        Var::new(atom.0)
    }

    /// Get the bound (threshold) for a rule.
    pub fn bound(&self, rule_idx: u32) -> Weight {
        self.rule_bounds[rule_idx as usize]
    }

    /// Get the sum of weights for a rule.
    pub fn sum_weights(&self, rule_idx: u32) -> Weight {
        self.rule_sum_weights[rule_idx as usize]
    }

    /// Get the active_r_cand variable for a rule index.
    pub fn active_cand(&self, rule_idx: u32) -> Var {
        Var::new(self.active_cand_base + rule_idx)
    }

    /// Get the active_r_check variable for a rule index.
    pub fn active_check(&self, rule_idx: u32) -> Var {
        Var::new(self.active_check_base + rule_idx)
    }

    /// Get the x_check variable for an atom.
    pub fn check(&self, atom: Atom) -> Var {
        Var::new(self.check_base + atom.0 - 1)
    }

    /// Get the x_dim variable for an atom.
    pub fn dim(&self, atom: Atom) -> Var {
        Var::new(self.dim_base + atom.0 - 1)
    }

    /// Get rules that have the given atom as a head.
    pub fn rules_for_head(&self, atom: Atom) -> &[HeadEntry] {
        self.atom_to_rules.get(&atom).map_or(&[], |v| v.as_slice())
    }

    /// Total number of variables.
    pub fn total_vars(&self) -> u32 {
        self.total_vars
    }

    /// Decode a raw variable number into what it represents.
    pub fn decode_var(&self, var_raw: u32) -> VarKind {
        if var_raw >= 1 && var_raw <= self.num_atoms {
            return VarKind::Cand(Atom(var_raw));
        }
        if var_raw >= self.active_cand_base && var_raw < self.active_cand_base + self.num_rules {
            return VarKind::ActiveCand {
                rule_idx: var_raw - self.active_cand_base,
            };
        }
        if var_raw >= self.active_check_base && var_raw < self.active_check_base + self.num_rules {
            return VarKind::ActiveCheck {
                rule_idx: var_raw - self.active_check_base,
            };
        }
        if var_raw >= self.check_base && var_raw < self.check_base + self.num_atoms {
            return VarKind::Check(Atom(var_raw - self.check_base + 1));
        }
        if var_raw >= self.dim_base && var_raw < self.dim_base + self.num_atoms {
            return VarKind::Dim(Atom(var_raw - self.dim_base + 1));
        }
        VarKind::Unknown(var_raw)
    }
}

/// What kind of variable a raw variable number represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarKind {
    Cand(Atom),
    ActiveCand { rule_idx: u32 },
    ActiveCheck { rule_idx: u32 },
    Check(Atom),
    Dim(Atom),
    Unknown(u32),
}

/// Encoded ASP program ready for solving.
pub struct EncodedProgram {
    pub layout: VarLayout,
    pub cand_clauses: Vec<Clause>,
    pub cand_pb_constraints: Vec<PBConstraint>,
    pub check_clauses: Vec<Clause>,
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
            Rule::Choice(r) => encode_choice_rule(
                r,
                rule_idx,
                &layout,
                &mut cand_pb_constraints,
                &mut check_clauses,
                &mut check_pb_constraints,
            ),
            Rule::Disjunctive(r) => encode_disjunctive_rule(
                r,
                rule_idx,
                &layout,
                &mut cand_clauses,
                &mut cand_pb_constraints,
                &mut check_clauses,
                &mut check_pb_constraints,
            ),
        }
    }

    // Add constraint that false atom (atom 1) is always false
    cand_clauses.push(vec![Lit::neg(layout.cand(Atom(1)))]);

    // Check solver constraints for each atom (Constraint 4):
    // (¬x_check, 1) + (x_cand, 1) + (¬x_dim, 1) >= 2
    for atom_id in 2..=layout.num_atoms {
        let atom = Atom(atom_id);
        let terms = vec![
            (Lit::neg(layout.check(atom)), 1),
            (Lit::pos(layout.cand(atom)), 1),
            (Lit::neg(layout.dim(atom)), 1),
        ];
        check_pb_constraints.push((terms, 2));
    }

    // Constraint 5: At least one atom must be diminished
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

/// Encode a choice rule: {h1, h2, ...} :- body
fn encode_choice_rule(
    rule: &ChoiceRule,
    rule_idx: u32,
    layout: &VarLayout,
    cand_pb_constraints: &mut Vec<PBConstraint>,
    check_clauses: &mut Vec<Clause>,
    check_pb_constraints: &mut Vec<PBConstraint>,
) {
    let active_cand = layout.active_cand(rule_idx);
    let active_check = layout.active_check(rule_idx);

    let sum_weights: Weight = rule.body.iter().map(|lit| lit.weight).sum();
    let bound = rule.bound;
    let falsification_weight = sum_weights - bound + 1;

    // Constraint 1: body satisfaction when active
    let mut c1_terms = vec![(Lit::pos(active_cand), falsification_weight)];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.cand(lit.atom))
        } else {
            Lit::pos(layout.cand(lit.atom))
        };
        c1_terms.push((cdcl_lit, lit.weight));
    }
    cand_pb_constraints.push((c1_terms, falsification_weight));

    // Constraint 2: body falsification when inactive
    let mut c2_terms = vec![(Lit::neg(active_cand), bound)];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::pos(layout.cand(lit.atom))
        } else {
            Lit::neg(layout.cand(lit.atom))
        };
        c2_terms.push((cdcl_lit, lit.weight));
    }
    cand_pb_constraints.push((c2_terms, bound));

    // Constraint 6: reduct body satisfaction
    let mut c6_terms = vec![
        (Lit::neg(active_cand), falsification_weight),
        (Lit::pos(active_check), falsification_weight),
    ];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.check(lit.atom))
        } else {
            Lit::pos(layout.check(lit.atom))
        };
        c6_terms.push((cdcl_lit, lit.weight));
    }
    check_pb_constraints.push((c6_terms, falsification_weight));

    // Constraint 8: reduct head propagation
    for &head in &rule.heads {
        check_clauses.push(vec![
            Lit::neg(layout.cand(head)),
            Lit::pos(layout.check(head)),
            Lit::neg(active_check),
        ]);
    }
}

/// Encode a disjunctive rule: h1 | h2 | ... :- body
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

    let sum_weights: Weight = rule.body.iter().map(|lit| lit.weight).sum();
    let bound = rule.bound;
    let falsification_weight = sum_weights - bound + 1;

    // Constraint 1: body satisfaction when active
    let mut c1_terms = vec![(Lit::pos(active_cand), falsification_weight)];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.cand(lit.atom))
        } else {
            Lit::pos(layout.cand(lit.atom))
        };
        c1_terms.push((cdcl_lit, lit.weight));
    }
    cand_pb_constraints.push((c1_terms, falsification_weight));

    // Constraint 2: body falsification when inactive
    let mut c2_terms = vec![(Lit::neg(active_cand), bound)];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::pos(layout.cand(lit.atom))
        } else {
            Lit::neg(layout.cand(lit.atom))
        };
        c2_terms.push((cdcl_lit, lit.weight));
    }
    cand_pb_constraints.push((c2_terms, bound));

    // Constraint 3: head requirement - if active, at least one head must be true
    let mut c3_clause: Vec<Lit> = rule
        .heads
        .iter()
        .map(|&h| Lit::pos(layout.cand(h)))
        .collect();
    c3_clause.push(Lit::neg(active_cand));
    cand_clauses.push(c3_clause);

    // Constraint 6: reduct body satisfaction
    let mut c6_terms = vec![
        (Lit::neg(active_cand), falsification_weight),
        (Lit::pos(active_check), falsification_weight),
    ];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            Lit::neg(layout.check(lit.atom))
        } else {
            Lit::pos(layout.check(lit.atom))
        };
        c6_terms.push((cdcl_lit, lit.weight));
    }
    if std::env::var("ASP_DEBUG_CONSTRAINTS").is_ok() {
        eprintln!(
            "Constraint 6 for disjunctive rule {}: heads={:?}, body={:?}, bound={}, F={}",
            rule_idx, rule.heads, rule.body, bound, falsification_weight
        );
        eprintln!("  c6_terms: {:?} >= {}", c6_terms, falsification_weight);
    }
    check_pb_constraints.push((c6_terms, falsification_weight));

    // Constraint 7: reduct head implication
    let mut c7_clause: Vec<Lit> = rule
        .heads
        .iter()
        .map(|&h| Lit::pos(layout.check(h)))
        .collect();
    c7_clause.push(Lit::neg(active_check));
    if std::env::var("ASP_DEBUG_CONSTRAINTS").is_ok() {
        eprintln!("Constraint 7 for rule {}: {:?}", rule_idx, c7_clause);
    }
    check_clauses.push(c7_clause);
}

/// Generate a loop constraint for a runtime unfounded set.
///
/// Uses the algorithm from ASP_formalization.md:
/// 1. If W_(not U) < t: skip rule (external support impossible)
/// 2. If non-UFS head exists and is TRUE: add ¬z (stealing head)
/// 3. Otherwise: add FALSE non-UFS body literals until weights sum to threshold
///
/// Uses PB constraint: reasons have weight k, UFS atoms have weight 1, bound = k
pub fn generate_loop_constraint(
    unfounded_set: &[Atom],
    program: &Program,
    layout: &VarLayout,
    assignment: &Multiset<Lit>,
) -> PBConstraint {
    let k = unfounded_set.len() as Weight;
    let u_set: HashSet<Atom> = unfounded_set.iter().copied().collect();

    let mut terms: Vec<(Lit, Weight)> = Vec::new();
    let mut seen_reasons: HashSet<Lit> = HashSet::new();

    // Add UFS atoms with weight 1
    for &atom in unfounded_set {
        terms.push((Lit::neg(layout.cand(atom)), 1));
    }

    // Use seeded RNG for deterministic shuffle
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    unfounded_set.hash(&mut hasher);
    let seed = hasher.finish();

    // Find reason literals for each rule that could support UFS atoms
    for &atom in unfounded_set {
        for entry in layout.rules_for_head(atom) {
            let active_var = layout.active_cand(entry.rule_idx);
            let active_val = assignment.contains(&Lit::pos(active_var));

            if !active_val {
                // Body not satisfied - reason is active_r
                let reason = Lit::pos(active_var);
                if seen_reasons.insert(reason) {
                    terms.push((reason, k));
                }
                continue;
            }

            // Body is satisfied - apply formalization algorithm
            let rule = &program.rules[entry.rule_idx as usize];
            let (heads, body, bound) = match rule {
                Rule::Disjunctive(r) => (&r.heads, &r.body, r.bound),
                Rule::Choice(r) => (&r.heads, &r.body, r.bound),
            };

            // W_(not U) = sum of weights of positive body literals NOT in U
            let w_not_u: Weight = body
                .iter()
                .filter(|lit| lit.positive && !u_set.contains(&lit.atom))
                .map(|lit| lit.weight)
                .sum();

            // Step 1: If W_(not U) < t, skip (external support impossible)
            if w_not_u < bound {
                continue;
            }

            // Step 2: If some head z is TRUE and not in U, add ¬z
            let mut found_stealing_head = false;
            for &head in heads {
                if u_set.contains(&head) {
                    continue;
                }
                let head_val = assignment.contains(&Lit::pos(layout.cand(head)));
                if head_val {
                    let reason = Lit::neg(layout.cand(head));
                    if seen_reasons.insert(reason) {
                        terms.push((reason, k));
                    }
                    found_stealing_head = true;
                    break;
                }
            }
            if found_stealing_head {
                continue;
            }

            // Step 3: Collect FALSE non-UFS positive body literals
            let mut false_lits: Vec<_> = body
                .iter()
                .filter(|lit| {
                    lit.positive
                        && !u_set.contains(&lit.atom)
                        && !assignment.contains(&Lit::pos(layout.cand(lit.atom)))
                })
                .collect();

            // Deterministic shuffle based on seed
            if seed & 1 == 1 {
                false_lits.reverse();
            }

            // Add until weights sum to at least W_(not U) - t + 1
            let threshold = w_not_u - bound + 1;
            let mut sum: Weight = 0;
            for lit in &false_lits {
                let reason = Lit::pos(layout.cand(lit.atom));
                if seen_reasons.insert(reason) {
                    terms.push((reason, k));
                }
                sum += lit.weight;
                if sum >= threshold {
                    break;
                }
            }

            // Step 4: If we couldn't reach threshold, panic
            if sum < threshold {
                eprintln!("DEBUG: Rule {} for UFS atom {:?}", entry.rule_idx, atom);
                eprintln!("  is_choice: {}", entry.is_choice);
                eprintln!("  heads: {:?}", heads);
                eprintln!("  body: {:?}", body);
                eprintln!(
                    "  bound: {}, w_not_u: {}, threshold: {}",
                    bound, w_not_u, threshold
                );
                eprintln!("  UFS: {:?}", u_set);
                for lit in body {
                    let var = layout.cand(lit.atom);
                    let val = assignment.contains(&Lit::pos(var));
                    let in_ufs = u_set.contains(&lit.atom);
                    eprintln!(
                        "    body lit {:?} (pos={}): in_ufs={}, val={}",
                        lit.atom, lit.positive, in_ufs, val
                    );
                }
                panic!(
                    "Bug: couldn't reach threshold {} (sum={}, w_not_u={}, bound={}, rule={})",
                    threshold, sum, w_not_u, bound, entry.rule_idx
                );
            }
        }
    }

    (terms, k)
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
        // 3 atoms, 2 basic rules
        let program = make_program(
            vec![
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(2)],
                    body: vec![],
                    bound: 0,
                }),
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(3)],
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

        assert_eq!(layout.total_vars(), 13);
    }

    #[test]
    fn test_encode_basic_rule() {
        let rule = DisjunctiveRule {
            heads: vec![Atom(2)],
            body: vec![WeightedLit::pos(Atom(3), 1), WeightedLit::neg(Atom(4), 1)],
            bound: 2,
        };

        let program = make_program(vec![Rule::Disjunctive(rule.clone())], 4);
        let layout = VarLayout::new(&program);

        let mut cand_clauses = Vec::new();
        let mut cand_pb = Vec::new();
        let mut check_clauses = Vec::new();
        let mut check_pb = Vec::new();
        encode_disjunctive_rule(
            &rule,
            0,
            &layout,
            &mut cand_clauses,
            &mut cand_pb,
            &mut check_clauses,
            &mut check_pb,
        );

        // Candidate: 1 clause (Constraint 3), 2 PB (Constraints 1 and 2)
        assert_eq!(cand_clauses.len(), 1);
        assert_eq!(cand_pb.len(), 2);

        // Check: 1 clause (Constraint 7), 1 PB (Constraint 6)
        assert_eq!(check_clauses.len(), 1);
        assert_eq!(check_pb.len(), 1);
    }
}
