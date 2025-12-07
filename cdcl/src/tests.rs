//! Tests for CDCL SAT solver.

use relational::database::DatabaseBuilder;
use relational::HashSet;

use super::types::{Lit, Var};
use super::*;

// Helper to create literals from raw i32
fn lit(raw: i32) -> Lit {
    Lit::from_raw(raw)
}

// Helper to create a set of variables
fn vars(ids: &[u32]) -> HashSet<Var> {
    ids.iter().map(|&n| Var::new(n)).collect()
}

#[test]
fn test_simple_sat() {
    // (x1 OR x2) AND (x1 OR NOT x2)
    // SAT: x1 = true
    let mut db_builder = DatabaseBuilder::new();
    let mut solver = Solver::new(&mut db_builder, &vars(&[1, 2]));
    let mut db = db_builder.build();

    solver.add_clause(&mut db, 0, &[lit(1), lit(2)]); // x1 OR x2
    solver.add_clause(&mut db, 1, &[lit(1), lit(-2)]); // x1 OR NOT x2

    assert!(solver.solve(&mut db));
    assert_eq!(solver.value(Var::new(1)), Some(true));
}

#[test]
fn test_simple_unsat() {
    // (x1) AND (NOT x1)
    // UNSAT
    let mut db_builder = DatabaseBuilder::new();
    let mut solver = Solver::new(&mut db_builder, &vars(&[1]));
    let mut db = db_builder.build();

    solver.add_clause(&mut db, 0, &[lit(1)]); // x1
    solver.add_clause(&mut db, 1, &[lit(-1)]); // NOT x1

    assert!(!solver.solve(&mut db));
}

#[test]
fn test_unit_propagation() {
    // (x1) AND (NOT x1 OR x2) AND (NOT x2 OR x3)
    // Unit prop: x1=T -> x2=T -> x3=T
    let mut db_builder = DatabaseBuilder::new();
    let mut solver = Solver::new(&mut db_builder, &vars(&[1, 2, 3]));
    let mut db = db_builder.build();

    solver.add_clause(&mut db, 0, &[lit(1)]); // x1
    solver.add_clause(&mut db, 1, &[lit(-1), lit(2)]); // NOT x1 OR x2
    solver.add_clause(&mut db, 2, &[lit(-2), lit(3)]); // NOT x2 OR x3

    assert!(solver.solve(&mut db));
    assert_eq!(solver.value(Var::new(1)), Some(true));
    assert_eq!(solver.value(Var::new(2)), Some(true));
    assert_eq!(solver.value(Var::new(3)), Some(true));
}

#[test]
fn test_backtracking() {
    // (x1 OR x2) AND (NOT x1 OR x2) AND (x1 OR NOT x2) AND (NOT x1 OR NOT x2)
    // This is UNSAT (pigeon hole for 2 pigeons, 1 hole)
    let mut db_builder = DatabaseBuilder::new();
    let mut solver = Solver::new(&mut db_builder, &vars(&[1, 2]));
    let mut db = db_builder.build();

    solver.add_clause(&mut db, 0, &[lit(1), lit(2)]); // x1 OR x2
    solver.add_clause(&mut db, 1, &[lit(-1), lit(2)]); // NOT x1 OR x2
    solver.add_clause(&mut db, 2, &[lit(1), lit(-2)]); // x1 OR NOT x2
    solver.add_clause(&mut db, 3, &[lit(-1), lit(-2)]); // NOT x1 OR NOT x2

    assert!(!solver.solve(&mut db));
}
