//! SAT encoding for ASP programs with two-solver architecture.
//!
//! Variable layout (multi-level for weight bodies):
//! - Atoms 1..N → x_cand (var 1..N)
//! - For each rule r, for each level s ∈ {t_r..W_r} → active_r,s_cand
//! - Rules 0..R-1 → active_r_check (one per rule)
//! - Atoms 1..N → x_check
//! - Atoms 1..N → x_dim
//! - For each non-choice rule r, for each level s → used_r,s_cand
//! - For each non-choice rule r, head h, level s → active_r,h,s_cand
//!
//! Number of levels per rule: W_r - t_r + 1 (where W_r = sum of weights, t_r = bound)

use std::collections::HashMap;

use cdcl::{Lit, Var, Weight};

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

/// Per-rule info for variable layout.
#[derive(Debug, Clone)]
struct RuleInfo {
    /// Bound (threshold) for this rule's body
    bound: Weight,
    /// Sum of all body weights
    sum_weights: Weight,
    /// Number of levels: sum_weights - bound + 1
    num_levels: u32,
    /// Starting offset for active_r,s_cand variables
    active_cand_start: u32,
    /// For non-choice rules only
    non_choice: Option<NonChoiceInfo>,
}

/// Additional info for non-choice rules.
#[derive(Debug, Clone)]
struct NonChoiceInfo {
    /// Number of heads
    num_heads: u32,
    /// Starting offset for used_r,s_cand variables
    used_cand_start: u32,
    /// Starting offset for active_r,h,s_cand variables
    active_head_cand_start: u32,
}

/// Variable layout for the ASP encoding.
#[derive(Debug, Clone)]
pub struct VarLayout {
    /// Number of atoms (N)
    pub num_atoms: u32,
    /// Number of rules (R)
    pub num_rules: u32,
    /// Info for each rule
    rule_info: Vec<RuleInfo>,
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
}

/// Helper to compute sum of weights for a body
fn sum_weights(body: &[WeightedLit]) -> Weight {
    body.iter().map(|lit| lit.weight).sum()
}

impl VarLayout {
    pub fn new(program: &Program) -> Self {
        let num_atoms = program.max_atom;
        let num_rules = program.rules.len() as u32;

        let mut rule_info = Vec::with_capacity(program.rules.len());
        let mut atom_to_rules: HashMap<Atom, Vec<HeadEntry>> = HashMap::new();

        // Phase 1: Compute active_cand offsets (all rules)
        // Layout: atoms (1..N), then active_r,s_cand for each rule and level
        let mut active_cand_offset = num_atoms + 1;

        for (rule_idx, rule) in program.rules.iter().enumerate() {
            let rule_idx_u32 = rule_idx as u32;
            let (bound, body, heads, is_choice) = match rule {
                Rule::Disjunctive(r) => (r.bound, &r.body, &r.heads, false),
                Rule::Choice(r) => (r.bound, &r.body, &r.heads, true),
            };

            let sw = sum_weights(body);
            let num_levels = (sw - bound + 1) as u32;

            // Build atom_to_rules index
            for (head_idx, &head) in heads.iter().enumerate() {
                atom_to_rules.entry(head).or_default().push(HeadEntry {
                    rule_idx: rule_idx_u32,
                    head_idx: head_idx as u32,
                    is_choice,
                });
            }

            rule_info.push(RuleInfo {
                bound,
                sum_weights: sw,
                num_levels,
                active_cand_start: active_cand_offset,
                non_choice: None, // Will be filled in phase 2
            });

            active_cand_offset += num_levels;
        }

        // After active_cand: active_check (one per rule)
        let active_check_base = active_cand_offset;
        let check_base = active_check_base + num_rules;
        let dim_base = check_base + num_atoms;

        // Phase 2: Compute used_cand and active_head_cand offsets (non-choice only)
        let mut used_cand_offset = dim_base + num_atoms;
        let mut active_head_offset = used_cand_offset;

        // First pass: count total used_cand variables
        for (rule_idx, rule) in program.rules.iter().enumerate() {
            if let Rule::Disjunctive(_) = rule {
                active_head_offset += rule_info[rule_idx].num_levels;
            }
        }

        // Second pass: assign offsets
        for (rule_idx, rule) in program.rules.iter().enumerate() {
            if let Rule::Disjunctive(r) = rule {
                let info = &mut rule_info[rule_idx];
                let num_heads = r.heads.len() as u32;
                let num_levels = info.num_levels;

                info.non_choice = Some(NonChoiceInfo {
                    num_heads,
                    used_cand_start: used_cand_offset,
                    active_head_cand_start: active_head_offset,
                });

                used_cand_offset += num_levels;
                active_head_offset += num_heads * num_levels;
            }
        }

        // total_vars is the highest variable number used (1-indexed)
        // active_head_offset points to the next slot after the last variable
        let total_vars = active_head_offset - 1;

        VarLayout {
            num_atoms,
            num_rules,
            rule_info,
            active_check_base,
            check_base,
            dim_base,
            total_vars,
            atom_to_rules,
        }
    }

    /// Get the x_cand variable for an atom.
    pub fn cand(&self, atom: Atom) -> Var {
        Var::new(atom.0)
    }

    /// Get the bound (threshold) for a rule.
    pub fn bound(&self, rule_idx: u32) -> Weight {
        self.rule_info[rule_idx as usize].bound
    }

    /// Get the sum of weights for a rule.
    pub fn sum_weights(&self, rule_idx: u32) -> Weight {
        self.rule_info[rule_idx as usize].sum_weights
    }

    /// Get the number of levels for a rule.
    pub fn num_levels(&self, rule_idx: u32) -> u32 {
        self.rule_info[rule_idx as usize].num_levels
    }

    /// Get the active_r,s_cand variable for a rule index and level.
    /// level must be in range [bound, sum_weights].
    pub fn active_cand(&self, rule_idx: u32, level: Weight) -> Var {
        let info = &self.rule_info[rule_idx as usize];
        let level_offset = (level - info.bound) as u32;
        assert!(level_offset < info.num_levels, "level out of range");
        Var::new(info.active_cand_start + level_offset)
    }

    /// Get the active_r_cand variable at base level (s = bound).
    pub fn active_cand_base(&self, rule_idx: u32) -> Var {
        let info = &self.rule_info[rule_idx as usize];
        Var::new(info.active_cand_start)
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

    /// Get the used_r,s_cand variable for a non-choice rule at a level.
    /// Panics if rule_idx is a choice rule.
    pub fn used_cand(&self, rule_idx: u32, level: Weight) -> Var {
        let info = &self.rule_info[rule_idx as usize];
        let nc = info.non_choice.as_ref().expect("used_cand called on choice rule");
        let level_offset = (level - info.bound) as u32;
        assert!(level_offset < info.num_levels, "level out of range");
        Var::new(nc.used_cand_start + level_offset)
    }

    /// Get the used_r_cand variable at base level (s = bound).
    pub fn used_cand_base(&self, rule_idx: u32) -> Var {
        let info = &self.rule_info[rule_idx as usize];
        let nc = info.non_choice.as_ref().expect("used_cand_base called on choice rule");
        Var::new(nc.used_cand_start)
    }

    /// Get the active_r,h,s_cand variable for a specific head of a non-choice rule at a level.
    /// head_idx is 0-based index into the rule's heads.
    /// Panics if rule_idx is a choice rule.
    pub fn active_head_cand(&self, rule_idx: u32, head_idx: u32, level: Weight) -> Var {
        let info = &self.rule_info[rule_idx as usize];
        let nc = info.non_choice.as_ref().expect("active_head_cand called on choice rule");
        assert!(head_idx < nc.num_heads, "head_idx out of range");
        let level_offset = (level - info.bound) as u32;
        assert!(level_offset < info.num_levels, "level out of range");
        // Layout: for each head, all levels are contiguous
        Var::new(nc.active_head_cand_start + head_idx * info.num_levels + level_offset)
    }

    /// Get the active_r,h_cand variable at base level (s = bound).
    pub fn active_head_cand_base(&self, rule_idx: u32, head_idx: u32) -> Var {
        let info = &self.rule_info[rule_idx as usize];
        let nc = info.non_choice.as_ref().expect("active_head_cand_base called on choice rule");
        assert!(head_idx < nc.num_heads, "head_idx out of range");
        Var::new(nc.active_head_cand_start + head_idx * info.num_levels)
    }

    /// Check if a rule is a non-choice rule (disjunctive).
    pub fn is_non_choice(&self, rule_idx: u32) -> bool {
        self.rule_info[rule_idx as usize].non_choice.is_some()
    }

    /// Get the number of heads for a non-choice rule.
    pub fn num_heads(&self, rule_idx: u32) -> u32 {
        self.rule_info[rule_idx as usize]
            .non_choice
            .as_ref()
            .map(|nc| nc.num_heads)
            .unwrap_or(0)
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
    /// Returns (kind, rule_idx, head_idx, level) where applicable.
    pub fn decode_var(&self, var_raw: u32) -> VarKind {
        // x_cand: 1..num_atoms
        if var_raw >= 1 && var_raw <= self.num_atoms {
            return VarKind::Cand(Atom(var_raw));
        }

        // active_cand: search through rule_info
        for (rule_idx, info) in self.rule_info.iter().enumerate() {
            let end = info.active_cand_start + info.num_levels;
            if var_raw >= info.active_cand_start && var_raw < end {
                let level_offset = var_raw - info.active_cand_start;
                let level = info.bound + level_offset as Weight;
                return VarKind::ActiveCand {
                    rule_idx: rule_idx as u32,
                    level,
                };
            }
        }

        // active_check: active_check_base + rule_idx
        if var_raw >= self.active_check_base
            && var_raw < self.active_check_base + self.num_rules
        {
            let rule_idx = var_raw - self.active_check_base;
            return VarKind::ActiveCheck { rule_idx };
        }

        // x_check: check_base + atom - 1
        if var_raw >= self.check_base && var_raw < self.check_base + self.num_atoms {
            let atom = Atom(var_raw - self.check_base + 1);
            return VarKind::Check(atom);
        }

        // x_dim: dim_base + atom - 1
        if var_raw >= self.dim_base && var_raw < self.dim_base + self.num_atoms {
            let atom = Atom(var_raw - self.dim_base + 1);
            return VarKind::Dim(atom);
        }

        // used_cand and active_head_cand: search through non-choice rules
        for (rule_idx, info) in self.rule_info.iter().enumerate() {
            if let Some(nc) = &info.non_choice {
                // used_cand
                let used_end = nc.used_cand_start + info.num_levels;
                if var_raw >= nc.used_cand_start && var_raw < used_end {
                    let level_offset = var_raw - nc.used_cand_start;
                    let level = info.bound + level_offset as Weight;
                    return VarKind::UsedCand {
                        rule_idx: rule_idx as u32,
                        level,
                    };
                }

                // active_head_cand: layout is head_idx * num_levels + level_offset
                let total_active_head = nc.num_heads * info.num_levels;
                let active_head_end = nc.active_head_cand_start + total_active_head;
                if var_raw >= nc.active_head_cand_start && var_raw < active_head_end {
                    let offset = var_raw - nc.active_head_cand_start;
                    let head_idx = offset / info.num_levels;
                    let level_offset = offset % info.num_levels;
                    let level = info.bound + level_offset as Weight;
                    return VarKind::ActiveHeadCand {
                        rule_idx: rule_idx as u32,
                        head_idx,
                        level,
                    };
                }
            }
        }

        VarKind::Unknown(var_raw)
    }
}

/// What kind of variable a raw variable number represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarKind {
    /// x_cand (atom in candidate)
    Cand(Atom),
    /// active_r,s_cand (rule active at level s)
    ActiveCand { rule_idx: u32, level: Weight },
    /// active_r_check (rule active in check)
    ActiveCheck { rule_idx: u32 },
    /// x_check (atom in check)
    Check(Atom),
    /// x_dim (diminished atom)
    Dim(Atom),
    /// used_r,s_cand (non-choice rule used at level s)
    UsedCand { rule_idx: u32, level: Weight },
    /// active_r,h,s_cand (non-choice rule head h active at level s)
    ActiveHeadCand {
        rule_idx: u32,
        head_idx: u32,
        level: Weight,
    },
    /// Unknown variable
    Unknown(u32),
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
        let atom = Atom(atom_id);
        let pb_constraint = generate_loop_constraint(atom, &[atom], program, &layout);
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
    let active_cand = layout.active_cand_base(rule_idx);
    let active_check = layout.active_check(rule_idx);

    // Calculate sum of weights
    let sum_weights: Weight = rule.body.iter().map(|lit| lit.weight).sum();
    let bound = rule.bound;

    // Falsification weight: W_r = sum - bound + 1
    let falsification_weight = sum_weights - bound + 1;

    // Constraint 1 (body satisfaction when active)
    // Per ASP_formalization.md: positive body → ¬b_i, negative body → c_j
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

    // Constraint 2 (body falsification when inactive) - at each level s
    // Per ASP_formalization.md Constraint 2: s · ¬active_r,s + ... >= s
    let num_levels = layout.num_levels(rule_idx);
    for level_offset in 0..num_levels {
        let level = bound + level_offset as Weight;
        let active_at_level = layout.active_cand(rule_idx, level);
        let mut constraint2_terms = vec![(Lit::neg(active_at_level), level)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                Lit::pos(layout.cand(lit.atom))
            } else {
                Lit::neg(layout.cand(lit.atom))
            };
            constraint2_terms.push((cdcl_lit, lit.weight));
        }
        cand_pb_constraints.push((constraint2_terms, level));
    }

    // NOTE: No Constraint 3 - heads are OPTIONAL in choice rules

    // Constraint 10 (reduct body satisfaction)
    // Same polarity as Constraint 1, but on check variables
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
    let active_cand = layout.active_cand_base(rule_idx);
    let active_check = layout.active_check(rule_idx);
    let n = rule.heads.len() as Weight;

    // Calculate sum of weights
    let sum_weights: Weight = rule.body.iter().map(|lit| lit.weight).sum();
    let bound = rule.bound;

    // Falsification weight: W_r = sum - bound + 1
    let falsification_weight = sum_weights - bound + 1;

    // Constraint 1 (body satisfaction when active):
    // W_r · active_r_cand + Σ w_i · ¬b_i_cand + Σ u_j · c_j_cand >= W_r
    // Per ASP_formalization.md: positive body → ¬b_i, negative body → c_j
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

    // Constraint 2 (body falsification when inactive) - at each level s:
    // s · ¬active_r,s_cand + Σ w_i · b_i_cand + Σ u_j · ¬c_j_cand >= s
    let num_levels = layout.num_levels(rule_idx);
    for level_offset in 0..num_levels {
        let level = bound + level_offset as Weight;
        let active_at_level = layout.active_cand(rule_idx, level);
        let mut constraint2_terms = vec![(Lit::neg(active_at_level), level)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                Lit::pos(layout.cand(lit.atom))
            } else {
                Lit::neg(layout.cand(lit.atom))
            };
            constraint2_terms.push((cdcl_lit, lit.weight));
        }
        cand_pb_constraints.push((constraint2_terms, level));
    }

    // Constraint 3 (head requirement) - base level: Σ h_cand + ¬active_r_cand >= 1
    // For integrity constraints (empty heads), this is just ¬active_r_cand >= 1
    let mut constraint3_clause: Vec<Lit> = rule
        .heads
        .iter()
        .map(|&h| Lit::pos(layout.cand(h)))
        .collect();
    constraint3_clause.push(Lit::neg(active_cand));
    cand_clauses.push(constraint3_clause);

    // Constraints 4 and 5 at each level s
    for level_offset in 0..num_levels {
        let level = bound + level_offset as Weight;
        let active_at_level = layout.active_cand(rule_idx, level);
        let used_at_level = layout.used_cand(rule_idx, level);

        // Constraint 4 (used implies active) - at each level: ¬used_r,s_cand ∨ active_r,s_cand
        cand_clauses.push(vec![Lit::neg(used_at_level), Lit::pos(active_at_level)]);

        // Constraint 5 (head selection) - at each level: Σ ¬active_r,h,s_cand + used_r,s_cand >= n
        let mut constraint5_terms: Vec<(Lit, Weight)> = (0..rule.heads.len())
            .map(|head_idx| {
                (
                    Lit::neg(layout.active_head_cand(rule_idx, head_idx as u32, level)),
                    1,
                )
            })
            .collect();
        constraint5_terms.push((Lit::pos(used_at_level), n));
        cand_pb_constraints.push((constraint5_terms, n));
    }

    // Constraint 6 (exclusive head) - base level: Σ ¬h_cand + (n-1)·¬used_r_cand >= n-1
    let used_cand_base = layout.used_cand_base(rule_idx);
    let mut constraint6_terms: Vec<(Lit, Weight)> = rule
        .heads
        .iter()
        .map(|&h| (Lit::neg(layout.cand(h)), 1))
        .collect();
    constraint6_terms.push((Lit::neg(used_cand_base), n - 1));
    cand_pb_constraints.push((constraint6_terms, n - 1));

    // Constraint 7 (head propagation): h_cand ∨ ¬active_r_h_cand (for each head)
    for (head_idx, &head) in rule.heads.iter().enumerate() {
        cand_clauses.push(vec![
            Lit::pos(layout.cand(head)),
            Lit::neg(layout.active_head_cand_base(rule_idx, head_idx as u32)),
        ]);
    }

    // Constraint 10 (reduct body satisfaction):
    // W_r · ¬active_r_cand + W_r · active_r_check + Σ w_i · ¬b_i_check + Σ u_j · c_j_check >= W_r
    // Same polarity as Constraint 1, but on check variables
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
    // For integrity constraints (empty heads), this is just ¬active_r_check >= 1
    let mut constraint11_clause: Vec<Lit> = rule
        .heads
        .iter()
        .map(|&h| Lit::pos(layout.check(h)))
        .collect();
    constraint11_clause.push(Lit::neg(active_check));
    check_clauses.push(constraint11_clause);
}

/// Generate a loop constraint for the candidate solver (Constraint 13).
///
/// Given an unfounded set U (atoms in candidate but not in check model),
/// find external support and require at least one to be active.
///
/// For weight bodies, a rule can provide external support even if some positive
/// body atoms are in U, as long as the remaining atoms can satisfy the bound.
/// We use level s = t + overlap_weight where overlap_weight is the sum of weights
/// for positive body atoms in U.
///
/// For choice rules, use active_r,s_cand.
/// For non-choice rules, use active_r,h,s_cand for each head h in U.
pub fn generate_loop_constraint(
    _chosen_atom: Atom,
    unfounded_set: &[Atom],
    program: &Program,
    layout: &VarLayout,
) -> PBConstraint {
    let u_set: std::collections::HashSet<Atom> = unfounded_set.iter().copied().collect();

    let mut terms: Vec<(Lit, Weight)> = Vec::new();

    // Add ALL UFS atoms as negated terms
    // Constraint: at least one UFS atom is false OR there's external support
    for &atom in unfounded_set {
        terms.push((Lit::neg(layout.cand(atom)), 1));
    }

    // Track which (choice_rule, level) pairs we've already added
    let mut added_choice_rules: std::collections::HashSet<(u32, Weight)> =
        std::collections::HashSet::new();

    // Find external support for ANY atom in the UFS
    for &atom in unfounded_set {
        for entry in layout.rules_for_head(atom) {
            let rule = &program.rules[entry.rule_idx as usize];
            let (body, bound) = match rule {
                Rule::Choice(r) => (&r.body, r.bound),
                Rule::Disjunctive(r) => (&r.body, r.bound),
            };

            // Calculate overlap_weight: sum of weights for positive body atoms in UFS
            let overlap_weight: Weight = body
                .iter()
                .filter(|lit| lit.positive && u_set.contains(&lit.atom))
                .map(|lit| lit.weight)
                .sum();

            // Compute the required level: s = t + overlap_weight
            let level = bound + overlap_weight;
            let sum_weights = layout.sum_weights(entry.rule_idx);

            // If s > W_r, rule cannot provide external support (skip)
            if level > sum_weights {
                if std::env::var("ASP_DEBUG_EXT").is_ok() && unfounded_set.len() > 1 {
                    eprintln!(
                        "c     Rule {} for {:?}: INTERNAL (level {} > sum_weights {})",
                        entry.rule_idx, atom, level, sum_weights
                    );
                }
                continue;
            }

            // Rule can provide external support at this level
            if entry.is_choice {
                // Choice rules use active_r,s_cand (deduplicate by rule+level)
                if added_choice_rules.insert((entry.rule_idx, level)) {
                    let var = layout.active_cand(entry.rule_idx, level);
                    if std::env::var("ASP_DEBUG_EXT").is_ok() && unfounded_set.len() > 1 {
                        eprintln!(
                            "c     Rule {} for {:?}: EXTERNAL (choice, level {}) -> var {}",
                            entry.rule_idx, atom, level, var.raw()
                        );
                    }
                    terms.push((Lit::pos(var), 1));
                }
            } else {
                // Non-choice rules use active_r,h,s_cand for this specific head
                let var = layout.active_head_cand(entry.rule_idx, entry.head_idx, level);
                if std::env::var("ASP_DEBUG_EXT").is_ok() && unfounded_set.len() > 1 {
                    eprintln!(
                        "c     Rule {} for {:?}: EXTERNAL (non-choice, level {}) -> var {}",
                        entry.rule_idx, atom, level, var.raw()
                    );
                }
                terms.push((Lit::pos(var), 1));
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

        // active_cand: 4, 5 (base level for each rule with 1 level)
        assert_eq!(layout.active_cand_base(0).raw(), 4);
        assert_eq!(layout.active_cand_base(1).raw(), 5);

        // active_check: 6, 7
        assert_eq!(layout.active_check(0).raw(), 6);
        assert_eq!(layout.active_check(1).raw(), 7);

        // check: 8, 9, 10
        assert_eq!(layout.check(Atom(1)).raw(), 8);
        assert_eq!(layout.check(Atom(3)).raw(), 10);

        // dim: 11, 12, 13
        assert_eq!(layout.dim(Atom(1)).raw(), 11);
        assert_eq!(layout.dim(Atom(3)).raw(), 13);

        // used_cand for 2 non-choice rules: 14, 15 (base level for each)
        assert_eq!(layout.used_cand_base(0).raw(), 14);
        assert_eq!(layout.used_cand_base(1).raw(), 15);

        // active_head_cand: each rule has 1 head, so: 16, 17 (base level for each)
        assert_eq!(layout.active_head_cand_base(0, 0).raw(), 16);
        assert_eq!(layout.active_head_cand_base(1, 0).raw(), 17);

        // Total: 3 atoms + 2 active_cand + 2 active_check + 3 check + 3 dim
        //        + 2 used_cand + 2 active_head_cand = 17
        assert_eq!(layout.total_vars(), 17);
    }

    #[test]
    fn test_encode_basic_rule() {
        // h :- b, not c  (atoms: h=2, b=3, c=4)
        // This is a basic rule with weights all 1 and bound = 2
        let rule = DisjunctiveRule {
            heads: vec![Atom(2)],
            body: vec![WeightedLit::pos(Atom(3), 1), WeightedLit::neg(Atom(4), 1)],
            bound: 2,
        };

        let program = make_program(vec![Rule::Disjunctive(rule.clone())], 4);
        let layout = VarLayout::new(&program);

        let mut cand = Vec::new();
        let mut cand_pb = Vec::new();
        let mut check = Vec::new();
        let mut check_pb = Vec::new();
        encode_disjunctive_rule(
            &rule,
            0,
            &layout,
            &mut cand,
            &mut cand_pb,
            &mut check,
            &mut check_pb,
        );

        // Candidate solver should have 3 clauses:
        // Constraint 3: h_cand ∨ ¬active_cand_0
        // Constraint 4: ¬used_cand ∨ active_cand
        // Constraint 7: h_cand ∨ ¬active_head_cand
        assert_eq!(cand.len(), 3);

        // Candidate solver should have 4 PB constraints:
        // Constraint 1: body satisfaction
        // Constraint 2: body falsification
        // Constraint 5: head selection (¬active_head_cand + used_cand >= 1)
        // Constraint 6: exclusive head (¬h_cand + 0·¬used_cand >= 0, trivial but still added)
        assert_eq!(cand_pb.len(), 4);

        // Check solver should have 1 clause (Constraint 11):
        // h_check ∨ ¬active_check_0
        assert_eq!(check.len(), 1);

        // Check solver should have 1 PB constraint (Constraint 10)
        assert_eq!(check_pb.len(), 1);
    }

    #[test]
    fn test_loop_constraint_cardinality_external_support() {
        // Program:
        //   aux :- {c, b} >= 1.   (cardinality rule)
        //   a :- aux.
        //   b :- a.
        //   {c}.
        //
        // With UFS = {a, b, aux} (atoms 2, 3, 4), the cardinality rule should
        // provide external support because c (atom 5) is NOT in UFS and
        // c alone satisfies the bound (weight 1 >= 1).
        //
        // Bug: current code checks "any positive body atom in UFS" which is true (b is there),
        // so it incorrectly marks the rule as INTERNAL.
        // Fix: should check "can body be satisfied without UFS atoms" which is YES (c alone works).

        let program = make_program(
            vec![
                // Rule 0: aux :- {c, b} >= 1 (cardinality rule as disjunctive with weight body)
                // Atoms: aux=2, b=3, c=4
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(2)], // aux
                    body: vec![
                        WeightedLit::pos(Atom(4), 1), // c with weight 1
                        WeightedLit::pos(Atom(3), 1), // b with weight 1
                    ],
                    bound: 1,
                }),
                // Rule 1: a :- aux
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(5)],                     // a
                    body: vec![WeightedLit::pos(Atom(2), 1)], // aux
                    bound: 1,
                }),
                // Rule 2: b :- a
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(3)],                     // b
                    body: vec![WeightedLit::pos(Atom(5), 1)], // a
                    bound: 1,
                }),
                // Rule 3: {c}. (choice rule)
                Rule::Choice(ChoiceRule {
                    heads: vec![Atom(4)], // c
                    body: vec![],
                    bound: 0,
                }),
            ],
            5, // max_atom
        );

        let layout = VarLayout::new(&program);

        // UFS = {aux, b, a} = {Atom(2), Atom(3), Atom(5)}
        // Chosen atom = aux = Atom(2)
        let unfounded_set = vec![Atom(2), Atom(3), Atom(5)];
        let chosen_atom = Atom(2);

        let (terms, bound) =
            generate_loop_constraint(chosen_atom, &unfounded_set, &program, &layout);

        // The constraint should be: ¬aux ∨ active_head_cand[rule0, aux]
        // Because rule 0 (aux :- {c, b} >= 1) CAN fire with just c (not in UFS).
        //
        // terms[0] should be the negated chosen atom
        // terms[1] should be the external support from rule 0
        assert_eq!(bound, 1);
        assert!(
            terms.len() > 1,
            "Expected external support for cardinality rule, but got only {} term(s): {:?}",
            terms.len(),
            terms
        );
    }
}
