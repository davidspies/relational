//! SCC (Strongly Connected Components) computation for ASP programs.
//!
//! Computes SCCs based on the positive dependency graph:
//! - Nodes: atoms
//! - Edges: head → positive body atom (head depends on body)
//!
//! SCCs are returned in topological order (dependencies before dependents).

use std::collections::HashMap;

use crate::types::{Atom, Program, Rule};

/// Information about SCCs in the program.
pub struct SccInfo {
    /// Which SCC each atom belongs to (0-indexed).
    /// Atoms not in any rule get their own singleton SCC.
    atom_to_scc: HashMap<Atom, usize>,
    /// The atoms in each SCC, in topological order (dependencies first).
    /// Each SCC contains at least one atom.
    sccs: Vec<Vec<Atom>>,
}

impl SccInfo {
    /// Get the SCC index for an atom.
    pub fn scc_of(&self, atom: Atom) -> usize {
        self.atom_to_scc[&atom]
    }

    /// Get all SCCs in topological order.
    pub fn sccs(&self) -> &[Vec<Atom>] {
        &self.sccs
    }

    /// Get the atoms in a specific SCC.
    pub fn atoms_in_scc(&self, scc_idx: usize) -> &[Atom] {
        &self.sccs[scc_idx]
    }

    /// Number of SCCs.
    pub fn num_sccs(&self) -> usize {
        self.sccs.len()
    }
}

/// Compute SCCs for the positive dependency graph of an ASP program.
///
/// Uses Tarjan's algorithm. Returns SCCs in topological order.
pub fn compute_sccs(program: &Program) -> SccInfo {
    // Build adjacency list for positive dependencies
    // Edge: head → positive body atom
    let mut adj: HashMap<Atom, Vec<Atom>> = HashMap::new();

    // Initialize all atoms (2 to max_atom)
    for atom_id in 2..=program.max_atom {
        adj.entry(Atom(atom_id)).or_default();
    }

    // Add edges from heads to positive body atoms
    for rule in &program.rules {
        let (heads, body) = match rule {
            Rule::Choice(r) => (&r.heads, &r.body),
            Rule::Disjunctive(r) => (&r.heads, &r.body),
        };

        for &head in heads {
            for lit in body {
                if lit.positive {
                    adj.entry(head).or_default().push(lit.atom);
                }
            }
        }
    }

    // Run Tarjan's algorithm
    let mut state = TarjanState::new();

    for atom_id in 2..=program.max_atom {
        let atom = Atom(atom_id);
        if !state.index.contains_key(&atom) {
            tarjan_visit(atom, &adj, &mut state);
        }
    }

    // Build result
    let mut atom_to_scc = HashMap::new();
    for (scc_idx, scc) in state.sccs.iter().enumerate() {
        for &atom in scc {
            atom_to_scc.insert(atom, scc_idx);
        }
    }

    SccInfo {
        atom_to_scc,
        sccs: state.sccs,
    }
}

struct TarjanState {
    index: HashMap<Atom, usize>,
    lowlink: HashMap<Atom, usize>,
    on_stack: HashMap<Atom, bool>,
    stack: Vec<Atom>,
    current_index: usize,
    sccs: Vec<Vec<Atom>>,
}

impl TarjanState {
    fn new() -> Self {
        Self {
            index: HashMap::new(),
            lowlink: HashMap::new(),
            on_stack: HashMap::new(),
            stack: Vec::new(),
            current_index: 0,
            sccs: Vec::new(),
        }
    }
}

fn tarjan_visit(v: Atom, adj: &HashMap<Atom, Vec<Atom>>, state: &mut TarjanState) {
    state.index.insert(v, state.current_index);
    state.lowlink.insert(v, state.current_index);
    state.current_index += 1;
    state.stack.push(v);
    state.on_stack.insert(v, true);

    // Visit successors
    if let Some(neighbors) = adj.get(&v) {
        for &w in neighbors {
            if !state.index.contains_key(&w) {
                // Not yet visited
                tarjan_visit(w, adj, state);
                let w_lowlink = state.lowlink[&w];
                let v_lowlink = state.lowlink.get_mut(&v).unwrap();
                *v_lowlink = (*v_lowlink).min(w_lowlink);
            } else if state.on_stack.get(&w).copied().unwrap_or(false) {
                // On stack, so part of current SCC
                let w_index = state.index[&w];
                let v_lowlink = state.lowlink.get_mut(&v).unwrap();
                *v_lowlink = (*v_lowlink).min(w_index);
            }
        }
    }

    // If v is a root, pop the SCC
    if state.lowlink[&v] == state.index[&v] {
        let mut scc = Vec::new();
        loop {
            let w = state.stack.pop().unwrap();
            state.on_stack.insert(w, false);
            scc.push(w);
            if w == v {
                break;
            }
        }
        // Sort for deterministic ordering
        scc.sort();
        state.sccs.push(scc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChoiceRule, DisjunctiveRule, WeightedLit};

    fn make_program(rules: Vec<Rule>, max_atom: u32) -> Program {
        Program {
            rules,
            symbols: Vec::new(),
            max_atom,
        }
    }

    #[test]
    fn test_simple_loop() {
        // a :- b. b :- a.
        // Both should be in the same SCC
        let program = make_program(
            vec![
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(2)], // a
                    body: vec![WeightedLit::pos(Atom(3), 1)], // b
                    bound: 1,
                }),
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(3)], // b
                    body: vec![WeightedLit::pos(Atom(2), 1)], // a
                    bound: 1,
                }),
            ],
            3,
        );

        let info = compute_sccs(&program);
        assert_eq!(info.scc_of(Atom(2)), info.scc_of(Atom(3)));
    }

    #[test]
    fn test_chain_no_loop() {
        // a. b :- a. c :- b.
        // Each should be in its own SCC
        let program = make_program(
            vec![
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(2)], // a
                    body: vec![],
                    bound: 0,
                }),
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(3)], // b
                    body: vec![WeightedLit::pos(Atom(2), 1)],
                    bound: 1,
                }),
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(4)], // c
                    body: vec![WeightedLit::pos(Atom(3), 1)],
                    bound: 1,
                }),
            ],
            4,
        );

        let info = compute_sccs(&program);
        // All should be in different SCCs
        assert_ne!(info.scc_of(Atom(2)), info.scc_of(Atom(3)));
        assert_ne!(info.scc_of(Atom(3)), info.scc_of(Atom(4)));
        assert_ne!(info.scc_of(Atom(2)), info.scc_of(Atom(4)));
    }

    #[test]
    fn test_negation_ignored() {
        // a :- not b. b :- not a.
        // These have no positive dependencies, so each is its own SCC
        let program = make_program(
            vec![
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(2)], // a
                    body: vec![WeightedLit::neg(Atom(3), 1)], // not b
                    bound: 1,
                }),
                Rule::Disjunctive(DisjunctiveRule {
                    heads: vec![Atom(3)], // b
                    body: vec![WeightedLit::neg(Atom(2), 1)], // not a
                    bound: 1,
                }),
            ],
            3,
        );

        let info = compute_sccs(&program);
        assert_ne!(info.scc_of(Atom(2)), info.scc_of(Atom(3)));
    }

    #[test]
    fn test_choice_rule_loop() {
        // {a} :- b. {b} :- a.
        // Same as regular loop
        let program = make_program(
            vec![
                Rule::Choice(ChoiceRule {
                    heads: vec![Atom(2)],
                    body: vec![WeightedLit::pos(Atom(3), 1)],
                    bound: 1,
                }),
                Rule::Choice(ChoiceRule {
                    heads: vec![Atom(3)],
                    body: vec![WeightedLit::pos(Atom(2), 1)],
                    bound: 1,
                }),
            ],
            3,
        );

        let info = compute_sccs(&program);
        assert_eq!(info.scc_of(Atom(2)), info.scc_of(Atom(3)));
    }
}
