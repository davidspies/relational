//! SAT encoding for ASP programs with two-solver architecture.
//!
//! Variable layout (multi-level for weight bodies):
//! - Atoms 1..N → x_cand (var 1..N)
//! - For each rule r, for each level s ∈ {t_r..W_r} → active_r,s_cand
//! - Rules 0..R-1 → active_r_check (one per rule)
//! - Atoms 1..N → x_check
//! - Atoms 1..N → x_dim
//!
//! Number of levels per rule: W_r - t_r + 1 (where W_r = sum of weights, t_r = bound)

use std::collections::HashMap;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use crate::types::{Atom, ChoiceRule, DisjunctiveRule, Program, Rule, WeightedLit};

/// A literal is a signed integer: positive for true, negative for false.
/// Variable n is represented as literal n (true) or -n (false).
pub type Lit = i32;

/// A variable is a positive integer.
pub type Var = i32;

/// Weight for PB constraints.
pub type Weight = i32;

/// Helper to create a positive literal from a variable.
pub fn pos(v: Var) -> Lit {
    v
}

/// Helper to create a negative literal from a variable.
pub fn neg(v: Var) -> Lit {
    -v
}

/// Entry in the atom→rules index for generate_loop_constraint.
#[derive(Debug, Clone, Copy)]
pub struct HeadEntry {
    pub rule_idx: u32,
    pub head_idx: u32,
    pub is_choice: bool,
}

/// A clause is a disjunction of literals.
pub type Clause = Vec<Lit>;

/// A PB constraint: sum of (lit * weight) >= bound, with classification.
#[derive(Debug, Clone)]
pub struct PBConstraint {
    pub terms: Vec<(Lit, Weight)>,
    pub bound: Weight,
    pub kind: ConstraintKind,
}

/// Classification of constraints from ASP_formalization.md
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    /// Constraint 1: Body Satisfaction (when active) - cand solver
    Constraint1 { rule_idx: u32, level: Weight },
    /// Constraint 2: Body Falsification (when inactive) - cand solver
    Constraint2 { rule_idx: u32, level: Weight },
    /// Constraint 3: Head Requirement (non-choice rules only) - cand solver
    Constraint3 { rule_idx: u32 },
    /// Constraint 4: Subset Relationship - check solver
    Constraint4 { atom: u32 },
    /// Constraint 5: Diminished Propagation - check solver (clause)
    Constraint5 { atom: u32 },
    /// Constraint 6: Strict Subset - check solver (clause)
    Constraint6,
    /// Constraint 7: Reduct Body Satisfaction - check solver
    Constraint7 { rule_idx: u32 },
    /// Constraint 8: Reduct Body Falsification - check solver
    Constraint8 { rule_idx: u32 },
    /// Constraint 9: Check Active Implies Cand Active - check solver (clause)
    Constraint9 { rule_idx: u32 },
    /// Constraint 10: Reduct Head Implication (non-choice rules) - check solver
    Constraint10 { rule_idx: u32 },
    /// Constraint 11: Reduct Head Propagation (choice rules) - check solver (clause)
    Constraint11 { rule_idx: u32, head: u32 },
    /// Constraint 12: Loop Constraint - cand solver
    Constraint12 { atom: u32 },
    /// False atom constraint (atom 1 must be false)
    FalseAtom,
}

impl std::fmt::Display for ConstraintKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConstraintKind::Constraint1 { rule_idx, level } => {
                write!(
                    f,
                    "Constraint 1 (Body Sat, rule={}, level={})",
                    rule_idx, level
                )
            }
            ConstraintKind::Constraint2 { rule_idx, level } => {
                write!(
                    f,
                    "Constraint 2 (Body Fals, rule={}, level={})",
                    rule_idx, level
                )
            }
            ConstraintKind::Constraint3 { rule_idx } => {
                write!(f, "Constraint 3 (Head Req, rule={})", rule_idx)
            }
            ConstraintKind::Constraint4 { atom } => {
                write!(f, "Constraint 4 (Subset, atom={})", atom)
            }
            ConstraintKind::Constraint5 { atom } => {
                write!(f, "Constraint 5 (Dim Prop, atom={})", atom)
            }
            ConstraintKind::Constraint6 => write!(f, "Constraint 6 (Strict Subset)"),
            ConstraintKind::Constraint7 { rule_idx } => {
                write!(f, "Constraint 7 (Reduct Body Sat, rule={})", rule_idx)
            }
            ConstraintKind::Constraint8 { rule_idx } => {
                write!(f, "Constraint 8 (Reduct Body Fals, rule={})", rule_idx)
            }
            ConstraintKind::Constraint9 { rule_idx } => {
                write!(f, "Constraint 9 (Check->Cand Active, rule={})", rule_idx)
            }
            ConstraintKind::Constraint10 { rule_idx } => {
                write!(f, "Constraint 10 (Reduct Head Impl, rule={})", rule_idx)
            }
            ConstraintKind::Constraint11 { rule_idx, head } => {
                write!(
                    f,
                    "Constraint 11 (Reduct Head Prop, rule={}, head={})",
                    rule_idx, head
                )
            }
            ConstraintKind::Constraint12 { atom } => {
                write!(f, "Constraint 12 (Loop, atom={})", atom)
            }
            ConstraintKind::FalseAtom => write!(f, "FalseAtom (atom 1 = false)"),
        }
    }
}

/// Normalize PB terms by combining duplicate literals.
/// Returns a new Vec with each literal appearing at most once, weights summed.
fn normalize_pb_terms(terms: Vec<(Lit, Weight)>) -> Vec<(Lit, Weight)> {
    let mut combined: HashMap<Lit, Weight> = HashMap::new();
    for (lit, weight) in terms {
        *combined.entry(lit).or_insert(0) += weight;
    }
    combined.into_iter().collect()
}

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
            });

            active_cand_offset += num_levels;
        }

        // After active_cand: active_check (one per rule)
        let active_check_base = active_cand_offset;
        let check_base = active_check_base + num_rules;
        let dim_base = check_base + num_atoms;

        // total_vars is the highest variable number used (1-indexed)
        let total_vars = dim_base + num_atoms - 1;

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
        atom.0 as Var
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
        (info.active_cand_start + level_offset) as Var
    }

    /// Get the active_r_cand variable at base level (s = bound).
    pub fn active_cand_base(&self, rule_idx: u32) -> Var {
        let info = &self.rule_info[rule_idx as usize];
        info.active_cand_start as Var
    }

    /// Get the active_r_check variable for a rule index.
    pub fn active_check(&self, rule_idx: u32) -> Var {
        (self.active_check_base + rule_idx) as Var
    }

    /// Get the x_check variable for an atom.
    pub fn check(&self, atom: Atom) -> Var {
        (self.check_base + atom.0 - 1) as Var
    }

    /// Get the x_dim variable for an atom.
    pub fn dim(&self, atom: Atom) -> Var {
        (self.dim_base + atom.0 - 1) as Var
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
        if var_raw >= self.active_check_base && var_raw < self.active_check_base + self.num_rules {
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
    cand_clauses.push(vec![neg(layout.cand(Atom(1)))]);

    // Add single-atom loop constraints for each atom (Constraint 12 initialization)
    // At initialization, no assignments exist, so use empty HashMap
    let empty_assignment: HashMap<Var, bool> = HashMap::new();
    let mut rng = StdRng::seed_from_u64(42);
    for atom_id in 2..=layout.num_atoms {
        let atom = Atom(atom_id);
        let pb_constraint = generate_loop_constraint(
            atom,
            &[atom],
            program,
            &layout,
            &empty_assignment,
            &mut rng,
            true,
            None, // no constraints available yet during initial setup
            None,
        );
        cand_pb_constraints.push(pb_constraint);
    }

    // Check solver constraints for each atom using PB constraint (Constraint 4):
    // (¬x_check, 1) + (x_cand, 1) + (¬x_dim, 1) >= 2
    // This encodes: x_check → (x_cand ∧ ¬x_dim)
    for atom_id in 2..=layout.num_atoms {
        let atom = Atom(atom_id);
        let terms = vec![
            (neg(layout.check(atom)), 1),
            (pos(layout.cand(atom)), 1),
            (neg(layout.dim(atom)), 1),
        ];
        check_pb_constraints.push(PBConstraint {
            terms,
            bound: 2,
            kind: ConstraintKind::Constraint4 { atom: atom_id },
        });
    }

    // Constraint 5 (Diminished Propagation): x_check + ¬x_cand + x_dim >= 1
    // This ensures that if x is in cand but not in check, then x_dim must be true.
    for atom_id in 2..=layout.num_atoms {
        let atom = Atom(atom_id);
        check_clauses.push(vec![
            pos(layout.check(atom)),
            neg(layout.cand(atom)),
            pos(layout.dim(atom)),
        ]);
    }

    // Constraint 6: At least one atom must be diminished (strict subset)
    // ∨ all x_dim
    // Note: if there are no user atoms, this is an empty clause (FALSE),
    // which correctly makes check solver UNSAT (no smaller model than {})
    let mut dim_clause = Vec::new();
    for atom_id in 2..=layout.num_atoms {
        dim_clause.push(pos(layout.dim(Atom(atom_id))));
    }
    check_clauses.push(dim_clause);

    // Constraint 9 (Check Active Implies Cand Active): active_r_cand + ¬active_r_check >= 1
    // This ensures that if a rule is active in check, it must also be active in cand.
    for rule_idx in 0..layout.num_rules {
        check_clauses.push(vec![
            pos(layout.active_cand_base(rule_idx)),
            neg(layout.active_check(rule_idx)),
        ]);
    }

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
/// Same as disjunctive rule but WITHOUT Constraint 3 (head requirement).
/// Heads are optional in choice rules.
///
/// Constraint 11 generates one clause per head: ¬h_cand ∨ h_check ∨ ¬active_r_check
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

    // Constraint 1 (body satisfaction when active) - at each level s:
    // F_s · active_r,s_cand + Σ w_i · ¬b_i_cand + Σ u_j · c_j_cand >= F_s
    // where F_s = W_r - s + 1 (falsification weight for level s)
    let num_levels = layout.num_levels(rule_idx);
    for level_offset in 0..num_levels {
        let level = bound + level_offset as Weight;
        let active_at_level = layout.active_cand(rule_idx, level);
        let falsification_weight = sum_weights - level + 1;
        let mut constraint1_terms = vec![(pos(active_at_level), falsification_weight)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                neg(layout.cand(lit.atom))
            } else {
                pos(layout.cand(lit.atom))
            };
            constraint1_terms.push((cdcl_lit, lit.weight));
        }
        cand_pb_constraints.push(PBConstraint {
            terms: normalize_pb_terms(constraint1_terms),
            bound: falsification_weight,
            kind: ConstraintKind::Constraint1 { rule_idx, level },
        });
    }

    // Constraint 2 (body falsification when inactive) - at each level s:
    // s · ¬active_r,s_cand + Σ w_i · b_i_cand + Σ u_j · ¬c_j_cand >= s
    for level_offset in 0..num_levels {
        let level = bound + level_offset as Weight;
        let active_at_level = layout.active_cand(rule_idx, level);
        let mut constraint2_terms = vec![(neg(active_at_level), level)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                pos(layout.cand(lit.atom))
            } else {
                neg(layout.cand(lit.atom))
            };
            constraint2_terms.push((cdcl_lit, lit.weight));
        }
        cand_pb_constraints.push(PBConstraint {
            terms: normalize_pb_terms(constraint2_terms),
            bound: level,
            kind: ConstraintKind::Constraint2 { rule_idx, level },
        });
    }

    // NOTE: No Constraint 3 - heads are OPTIONAL in choice rules

    // Constraint 7 (reduct body satisfaction)
    // Positive body uses check variables, negative body uses cand variables
    let falsification_weight = sum_weights - bound + 1;
    let mut constraint7_terms = vec![
        (neg(active_cand), falsification_weight),
        (pos(active_check), falsification_weight),
    ];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            neg(layout.check(lit.atom))
        } else {
            // Negative body uses cand, not check (per formalization Constraint 7)
            pos(layout.cand(lit.atom))
        };
        constraint7_terms.push((cdcl_lit, lit.weight));
    }
    check_pb_constraints.push(PBConstraint {
        terms: normalize_pb_terms(constraint7_terms),
        bound: falsification_weight,
        kind: ConstraintKind::Constraint7 { rule_idx },
    });

    // Constraint 8 (reduct body falsification):
    // t_r · ¬active_r_check + Σ w_i · b_i_check + Σ u_j · ¬c_j_cand >= t_r
    let mut constraint8_terms = vec![(neg(active_check), bound)];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            pos(layout.check(lit.atom))
        } else {
            // Negative body uses cand, not check (per formalization Constraint 8)
            neg(layout.cand(lit.atom))
        };
        constraint8_terms.push((cdcl_lit, lit.weight));
    }
    check_pb_constraints.push(PBConstraint {
        terms: normalize_pb_terms(constraint8_terms),
        bound,
        kind: ConstraintKind::Constraint8 { rule_idx },
    });

    // Constraint 11 (reduct head propagation): ¬h_cand ∨ h_check ∨ ¬active_r_check (for each head)
    for &head in &rule.heads {
        check_clauses.push(vec![
            neg(layout.cand(head)),
            pos(layout.check(head)),
            neg(active_check),
        ]);
    }
}

/// Encode a disjunctive rule: h1 | h2 | ... :- #sum{w1:b1; w2:b2; ...} >= bound
///
/// Candidate Constraint 1 (body satisfaction when active) - at each level s:
/// F_s · active_r,s_cand + Σ w_i · ¬b_i_cand + Σ u_j · c_j_cand >= F_s
/// where F_s = W_r - s + 1 (falsification weight for level s)
///
/// Candidate Constraint 2 (body falsification when inactive) - at each level s:
/// s · ¬active_r,s_cand + Σ w_i · b_i_cand + Σ u_j · ¬c_j_cand >= s
///
/// Candidate Constraint 3 (head requirement) - base level only:
/// Σ h_cand + ¬active_r_cand >= 1
///
/// Check Constraint 7 (reduct body satisfaction):
/// F_r · ¬active_r_cand + F_r · active_r_check + Σ w_i · ¬b_i_check + Σ u_j · c_j_cand >= F_r
///
/// Check Constraint 10 (reduct head implication):
/// Σ h_check + ¬active_r_check >= 1
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

    // Calculate sum of weights
    let sum_weights: Weight = rule.body.iter().map(|lit| lit.weight).sum();
    let bound = rule.bound;

    // Constraint 1 (body satisfaction when active) - at each level s:
    // F_s · active_r,s_cand + Σ w_i · ¬b_i_cand + Σ u_j · c_j_cand >= F_s
    // where F_s = W_r - s + 1 (falsification weight for level s)
    let num_levels = layout.num_levels(rule_idx);
    for level_offset in 0..num_levels {
        let level = bound + level_offset as Weight;
        let active_at_level = layout.active_cand(rule_idx, level);
        let falsification_weight = sum_weights - level + 1;
        let mut constraint1_terms = vec![(pos(active_at_level), falsification_weight)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                neg(layout.cand(lit.atom))
            } else {
                pos(layout.cand(lit.atom))
            };
            constraint1_terms.push((cdcl_lit, lit.weight));
        }
        cand_pb_constraints.push(PBConstraint {
            terms: normalize_pb_terms(constraint1_terms),
            bound: falsification_weight,
            kind: ConstraintKind::Constraint1 { rule_idx, level },
        });
    }

    // Constraint 2 (body falsification when inactive) - at each level s:
    // s · ¬active_r,s_cand + Σ w_i · b_i_cand + Σ u_j · ¬c_j_cand >= s
    for level_offset in 0..num_levels {
        let level = bound + level_offset as Weight;
        let active_at_level = layout.active_cand(rule_idx, level);
        let mut constraint2_terms = vec![(neg(active_at_level), level)];
        for lit in &rule.body {
            let cdcl_lit = if lit.positive {
                pos(layout.cand(lit.atom))
            } else {
                neg(layout.cand(lit.atom))
            };
            constraint2_terms.push((cdcl_lit, lit.weight));
        }
        cand_pb_constraints.push(PBConstraint {
            terms: normalize_pb_terms(constraint2_terms),
            bound: level,
            kind: ConstraintKind::Constraint2 { rule_idx, level },
        });
    }

    // Constraint 3 (head requirement) - base level: Σ h_cand + ¬active_r_cand >= 1
    // For integrity constraints (empty heads), this is just ¬active_r_cand >= 1
    let mut constraint3_clause: Vec<Lit> =
        rule.heads.iter().map(|&h| pos(layout.cand(h))).collect();
    constraint3_clause.push(neg(active_cand));
    cand_clauses.push(constraint3_clause);

    // Constraint 7 (reduct body satisfaction):
    // F_r · ¬active_r_cand + F_r · active_r_check + Σ w_i · ¬b_i_check + Σ u_j · c_j_cand >= F_r
    // Positive body uses check variables, negative body uses cand variables
    let falsification_weight = sum_weights - bound + 1;
    let mut constraint7_terms = vec![
        (neg(active_cand), falsification_weight),
        (pos(active_check), falsification_weight),
    ];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            neg(layout.check(lit.atom))
        } else {
            // Negative body uses cand, not check (per formalization Constraint 7)
            pos(layout.cand(lit.atom))
        };
        constraint7_terms.push((cdcl_lit, lit.weight));
    }
    check_pb_constraints.push(PBConstraint {
        terms: normalize_pb_terms(constraint7_terms),
        bound: falsification_weight,
        kind: ConstraintKind::Constraint7 { rule_idx },
    });

    // Constraint 8 (reduct body falsification):
    // t_r · ¬active_r_check + Σ w_i · b_i_check + Σ u_j · ¬c_j_cand >= t_r
    let mut constraint8_terms = vec![(neg(active_check), bound)];
    for lit in &rule.body {
        let cdcl_lit = if lit.positive {
            pos(layout.check(lit.atom))
        } else {
            // Negative body uses cand, not check (per formalization Constraint 8)
            neg(layout.cand(lit.atom))
        };
        constraint8_terms.push((cdcl_lit, lit.weight));
    }
    check_pb_constraints.push(PBConstraint {
        terms: normalize_pb_terms(constraint8_terms),
        bound,
        kind: ConstraintKind::Constraint8 { rule_idx },
    });

    // Constraint 10 (reduct head implication): Σ h_check + ¬active_r_check >= 1
    // For integrity constraints (empty heads), this is just ¬active_r_check >= 1
    let mut constraint10_clause: Vec<Lit> =
        rule.heads.iter().map(|&h| pos(layout.check(h))).collect();
    constraint10_clause.push(neg(active_check));
    check_clauses.push(constraint10_clause);
}

/// Generate a loop constraint for the candidate solver (Constraint 12).
///
/// Given an unfounded set U (atoms in candidate but not in check model),
/// find external support and require at least one to be active.
///
/// For weight bodies, a rule can provide external support even if some positive
/// body atoms are in U, as long as the remaining atoms can satisfy the bound.
/// We use level s = t + overlap_weight where overlap_weight is the sum of weights
/// for positive body atoms in U.
///
/// Both choice and non-choice rules use active_r,s_cand at the appropriate level.
pub fn generate_loop_constraint<R: rand::Rng>(
    chosen_atom: Atom,
    unfounded_set: &[Atom],
    program: &Program,
    layout: &VarLayout,
    assignment: &HashMap<Var, bool>,
    rng: &mut R,
    is_initial_setup: bool,
    cand_constraints: Option<&[PBConstraint]>,
    check_constraints: Option<&[PBConstraint]>,
) -> PBConstraint {
    let u_set: std::collections::HashSet<Atom> = unfounded_set.iter().copied().collect();

    // Collect reason literals and overlap atoms
    let mut reason_terms: Vec<Lit> = Vec::new();
    let mut seen_reasons: std::collections::HashSet<Lit> = std::collections::HashSet::new();
    let mut overlap_atoms: std::collections::HashSet<Atom> = std::collections::HashSet::new();

    // Track which (rule, level) pairs we've already added
    let mut added_rule_levels: std::collections::HashSet<(u32, Weight)> =
        std::collections::HashSet::new();

    // Find external support for ANY atom in the UFS
    for &atom in unfounded_set {
        for entry in layout.rules_for_head(atom) {
            let rule = &program.rules[entry.rule_idx as usize];
            let (heads, body, bound) = match rule {
                Rule::Choice(r) => (&r.heads, &r.body, r.bound),
                Rule::Disjunctive(r) => (&r.heads, &r.body, r.bound),
            };

            // Calculate overlap: positive body atoms in UFS
            let overlap_body_atoms: Vec<Atom> = body
                .iter()
                .filter(|lit| lit.positive && u_set.contains(&lit.atom))
                .map(|lit| lit.atom)
                .collect();

            let overlap_weight: Weight = body
                .iter()
                .filter(|lit| lit.positive && u_set.contains(&lit.atom))
                .map(|lit| lit.weight)
                .sum();

            // Compute the required level: s = t + overlap_weight
            let level = bound + overlap_weight;
            let sum_weights = layout.sum_weights(entry.rule_idx);

            // If s > W_r, rule cannot provide external support (skip, do NOT add to O)
            if level > sum_weights {
                continue;
            }

            // Check if active_{r,s,cand} is true
            let active_var = layout.active_cand(entry.rule_idx, level);
            let active_is_true = match assignment.get(&active_var).copied() {
                Some(_) if is_initial_setup => {
                    panic!(
                        "BUG: active_var {} is assigned during initial setup (expected unassigned)",
                        active_var
                    );
                }
                Some(v) => v,
                None if is_initial_setup => false, // During initial setup, unassigned is expected
                None => {
                    eprintln!(
                        "BUG: active_var {} not in assignment (rule_idx={}, level={})",
                        active_var, entry.rule_idx, level
                    );
                    eprintln!(
                        "  assignment has {} entries, max key = {:?}",
                        assignment.len(),
                        assignment.keys().max()
                    );
                    eprintln!(
                        "  num_atoms={}, num_rules={}",
                        layout.num_atoms, layout.num_rules
                    );
                    panic!("active_cand should have value");
                }
            };

            if !active_is_true {
                // Body isn't satisfied - add active as reason AND add overlap atoms to O
                if added_rule_levels.insert((entry.rule_idx, level)) {
                    let reason = pos(active_var);
                    if seen_reasons.insert(reason) {
                        reason_terms.push(reason);
                    }
                    // Add overlap atoms to O (only when reason is active variable)
                    for &overlap_atom in &overlap_body_atoms {
                        overlap_atoms.insert(overlap_atom);
                    }
                }
                continue;
            }

            // active is true - rule provides external support
            // For choice rules, this means U is not unfounded
            let stealing_heads: Vec<Atom> = if entry.is_choice {
                vec![]
            } else {
                // Non-choice rule with active=true - must have a stealing head
                heads
                    .iter()
                    .copied()
                    .filter(|&head| {
                        if u_set.contains(&head) {
                            return false; // Skip UFS heads
                        }
                        let head_var = layout.cand(head);
                        match assignment.get(&head_var).copied() {
                            Some(_) if is_initial_setup => {
                                panic!("BUG: head_var {} is assigned during initial setup (expected unassigned)", head_var);
                            }
                            Some(v) => v,
                            None if is_initial_setup => false, // During initial setup, unassigned is expected
                            None => panic!("head cand {} should have value", head_var),
                        }
                    })
                    .collect()
            };

            if stealing_heads.is_empty() {
                // Body is satisfied, no stealing head - U is not unfounded!
                eprintln!("=== PANIC DEBUG ===");
                eprintln!(
                    "rule_idx: {}, is_choice: {}",
                    entry.rule_idx, entry.is_choice
                );
                eprintln!("atom: {:?}, level: {}, bound: {}", atom, level, bound);
                eprintln!("heads: {:?}", heads);

                // Collect all relevant atoms for constraint lookup
                let mut relevant_atoms: Vec<Atom> = Vec::new();

                eprintln!("body ({} literals):", body.len());
                for lit in body {
                    let cand_var = layout.cand(lit.atom);
                    let check_var = layout.check(lit.atom);
                    let cand_val = assignment.get(&cand_var).copied();
                    let check_val = assignment.get(&check_var).copied();
                    eprintln!(
                        "  {} Atom({}) weight {} -> cand_var {}={:?}, check_var {}={:?}, in_ufs={}",
                        if lit.positive { "+" } else { "-" },
                        lit.atom.0,
                        lit.weight,
                        cand_var,
                        cand_val,
                        check_var,
                        check_val,
                        u_set.contains(&lit.atom)
                    );
                    relevant_atoms.push(lit.atom);
                }

                eprintln!("heads ({} atoms):", heads.len());
                for &head in heads {
                    let cand_var = layout.cand(head);
                    let check_var = layout.check(head);
                    let cand_val = assignment.get(&cand_var).copied();
                    let check_val = assignment.get(&check_var).copied();
                    eprintln!(
                        "  Atom({}) -> cand_var {}={:?}, check_var {}={:?}, in_ufs={}",
                        head.0,
                        cand_var,
                        cand_val,
                        check_var,
                        check_val,
                        u_set.contains(&head)
                    );
                    relevant_atoms.push(head);
                }

                // Print constraints containing relevant atoms
                if let Some(cand_cs) = cand_constraints {
                    eprintln!("\nCand constraints containing relevant atoms:");
                    for (idx, c) in cand_cs.iter().enumerate() {
                        let vars_in_constraint: Vec<Var> =
                            c.terms.iter().map(|(lit, _)| lit.abs()).collect();
                        let contains_relevant = relevant_atoms
                            .iter()
                            .any(|&a| vars_in_constraint.contains(&layout.cand(a)));
                        if contains_relevant {
                            eprintln!("  [{}] {} (bound={})", idx, c.kind, c.bound);
                            for &(lit, weight) in &c.terms {
                                let var = lit.abs();
                                let val = assignment.get(&var).copied();
                                let decoded = layout.decode_var(var as u32);
                                eprintln!(
                                    "      lit {} (var {} = {:?}) weight {} val={:?}",
                                    lit, var, decoded, weight, val
                                );
                            }
                        }
                    }
                }

                if let Some(check_cs) = check_constraints {
                    eprintln!("\nCheck constraints containing relevant atoms:");
                    for (idx, c) in check_cs.iter().enumerate() {
                        let vars_in_constraint: Vec<Var> =
                            c.terms.iter().map(|(lit, _)| lit.abs()).collect();
                        let contains_relevant = relevant_atoms.iter().any(|&a| {
                            vars_in_constraint.contains(&layout.cand(a))
                                || vars_in_constraint.contains(&layout.check(a))
                        });
                        if contains_relevant {
                            eprintln!("  [{}] {} (bound={})", idx, c.kind, c.bound);
                            for &(lit, weight) in &c.terms {
                                let var = lit.abs();
                                let val = assignment.get(&var).copied();
                                let decoded = layout.decode_var(var as u32);
                                eprintln!(
                                    "      lit {} (var {} = {:?}) weight {} val={:?}",
                                    lit, var, decoded, weight, val
                                );
                            }
                        }
                    }
                }

                panic!(
                    "Bug: unfounded set {:?} is not unfounded. \
                     Rule {} (is_choice={}) has atom {:?} in head, body satisfied at level {}, \
                     but no non-UFS stealing head is true.",
                    unfounded_set, entry.rule_idx, entry.is_choice, atom, level
                );
            }

            // Select one stealing head at random (do NOT add to O)
            let idx = rng.random_range(0..stealing_heads.len());
            let stealing_head = stealing_heads[idx];
            let reason = neg(layout.cand(stealing_head));
            if seen_reasons.insert(reason) {
                reason_terms.push(reason);
            }
        }
    }

    // Build the final constraint based on whether O is empty
    let kind = ConstraintKind::Constraint12 {
        atom: chosen_atom.0,
    };
    if !overlap_atoms.is_empty() {
        // O ≠ ∅: sum(¬x for x in O) + sum(reason_r) >= 1
        let mut terms: Vec<(Lit, Weight)> = Vec::new();
        for &atom in &overlap_atoms {
            terms.push((neg(layout.cand(atom)), 1));
        }
        for reason in reason_terms {
            terms.push((reason, 1));
        }
        PBConstraint {
            terms,
            bound: 1,
            kind,
        }
    } else {
        // O = ∅: sum(¬x for x in U) + sum(n * reason_r) >= n
        let n = unfounded_set.len() as Weight;
        let mut terms: Vec<(Lit, Weight)> = Vec::new();
        for &atom in unfounded_set {
            terms.push((neg(layout.cand(atom)), 1));
        }
        for reason in reason_terms {
            terms.push((reason, n));
        }
        PBConstraint {
            terms,
            bound: n,
            kind,
        }
    }
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
        // 3 atoms, 2 basic rules with single heads (empty body, bound=0, so 1 level each)
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
        assert_eq!(layout.cand(Atom(1)), 1);
        assert_eq!(layout.cand(Atom(3)), 3);

        // active_cand: 4, 5 (base level for each rule with 1 level)
        assert_eq!(layout.active_cand_base(0), 4);
        assert_eq!(layout.active_cand_base(1), 5);

        // active_check: 6, 7
        assert_eq!(layout.active_check(0), 6);
        assert_eq!(layout.active_check(1), 7);

        // check: 8, 9, 10
        assert_eq!(layout.check(Atom(1)), 8);
        assert_eq!(layout.check(Atom(3)), 10);

        // dim: 11, 12, 13
        assert_eq!(layout.dim(Atom(1)), 11);
        assert_eq!(layout.dim(Atom(3)), 13);

        // Total: 3 atoms + 2 active_cand + 2 active_check + 3 check + 3 dim = 13
        assert_eq!(layout.total_vars(), 13);
    }

    #[test]
    fn test_encode_basic_rule() {
        // h :- b, not c  (atoms: h=2, b=3, c=4)
        // This is a basic rule with weights all 1 and bound = 2
        // sum_weights = 2, num_levels = 2 - 2 + 1 = 1
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

        // Candidate solver should have 1 clause:
        // Constraint 3: h_cand ∨ ¬active_cand
        assert_eq!(cand.len(), 1);

        // Candidate solver should have 2 PB constraints (1 level each):
        // Constraint 1: body satisfaction at level 2
        // Constraint 2: body falsification at level 2
        assert_eq!(cand_pb.len(), 2);

        // Check solver should have 1 clause:
        // Constraint 10: h_check ∨ ¬active_check
        assert_eq!(check.len(), 1);

        // Check solver should have 2 PB constraints:
        // Constraint 7: reduct body satisfaction
        // Constraint 8: reduct body falsification
        assert_eq!(check_pb.len(), 2);
    }

    #[test]
    fn test_loop_constraint_cardinality_external_support() {
        // Program:
        //   aux :- {c, b} >= 1.   (cardinality rule)
        //   a :- aux.
        //   b :- a.
        //   {c}.
        //
        // With UFS = {a, b, aux} = {Atom(5), Atom(3), Atom(2)}:
        // - Rule 0 (aux :- {c, b} >= 1): overlap = {b}, level = 1+1=2 <= W_r=2, adds active and b to O
        // - Rule 1 (a :- aux): overlap = {aux}, level = 1+1=2 > W_r=1, SKIPPED
        // - Rule 2 (b :- a): overlap = {a}, level = 1+1=2 > W_r=1, SKIPPED
        //
        // Result: O = {b}, one reason (active_{0,2})
        // Constraint: ¬b + active_{0,2} >= 1

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

        // At test time, use empty assignment (no stealing heads)
        let empty_assignment: HashMap<Var, bool> = HashMap::new();
        let mut rng = StdRng::seed_from_u64(42);
        let pb = generate_loop_constraint(
            chosen_atom,
            &unfounded_set,
            &program,
            &layout,
            &empty_assignment,
            &mut rng,
            true, // is_initial_setup - using empty assignment
            None,
            None,
        );

        // O = {b} (only rule 0 contributes), plus one reason term
        // Constraint: ¬b + active_{0,2} >= 1
        assert_eq!(pb.bound, 1);
        assert_eq!(
            pb.terms.len(),
            2,
            "Expected 2 terms (1 overlap atom + 1 reason), got {:?}",
            pb.terms
        );

        // Verify we have the overlap atom (¬b = neg(Var(3)))
        let has_neg_b = pb
            .terms
            .iter()
            .any(|(lit, _)| *lit == neg(layout.cand(Atom(3))));
        assert!(has_neg_b, "Expected ¬b in terms, got {:?}", pb.terms);

        // Verify we have the active variable reason
        let has_active = pb.terms.iter().any(|(lit, _)| *lit > 0);
        assert!(
            has_active,
            "Expected active variable in terms, got {:?}",
            pb.terms
        );
    }
}
