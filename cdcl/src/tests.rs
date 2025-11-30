//! Tests for CDCL SAT solver.

use super::*;

// Helper to create literals from raw i32
fn lit(raw: i32) -> Lit {
    Lit::from_raw(raw)
}

// Helper to create clause IDs
fn cid(n: u32) -> ClauseId {
    ClauseId::new(n)
}

#[test]
fn test_simple_sat() {
    // (x1 OR x2) AND (x1 OR NOT x2)
    // SAT: x1 = true
    let mut solver = Solver::new(Var::new(2));
    solver.add_clause(cid(1), &[lit(1), lit(2)]); // x1 OR x2
    solver.add_clause(cid(2), &[lit(1), lit(-2)]); // x1 OR NOT x2

    assert!(solver.solve());
    assert_eq!(solver.value(Var::new(1)), Some(true));
}

#[test]
fn test_simple_unsat() {
    // (x1) AND (NOT x1)
    // UNSAT
    let mut solver = Solver::new(Var::new(1));
    solver.add_clause(cid(1), &[lit(1)]); // x1
    solver.add_clause(cid(2), &[lit(-1)]); // NOT x1

    assert!(!solver.solve());
}

#[test]
fn test_unit_propagation() {
    // (x1) AND (NOT x1 OR x2) AND (NOT x2 OR x3)
    // Unit prop: x1=T -> x2=T -> x3=T
    let mut solver = Solver::new(Var::new(3));
    solver.add_clause(cid(1), &[lit(1)]); // x1
    solver.add_clause(cid(2), &[lit(-1), lit(2)]); // NOT x1 OR x2
    solver.add_clause(cid(3), &[lit(-2), lit(3)]); // NOT x2 OR x3

    assert!(solver.solve());
    assert_eq!(solver.value(Var::new(1)), Some(true));
    assert_eq!(solver.value(Var::new(2)), Some(true));
    assert_eq!(solver.value(Var::new(3)), Some(true));
}

#[test]
fn test_backtracking() {
    // (x1 OR x2) AND (NOT x1 OR x2) AND (x1 OR NOT x2) AND (NOT x1 OR NOT x2)
    // This is UNSAT (pigeon hole for 2 pigeons, 1 hole)
    let mut solver = Solver::new(Var::new(2));
    solver.add_clause(cid(1), &[lit(1), lit(2)]); // x1 OR x2
    solver.add_clause(cid(2), &[lit(-1), lit(2)]); // NOT x1 OR x2
    solver.add_clause(cid(3), &[lit(1), lit(-2)]); // x1 OR NOT x2
    solver.add_clause(cid(4), &[lit(-1), lit(-2)]); // NOT x1 OR NOT x2

    assert!(!solver.solve());
}
