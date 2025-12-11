//! Tests for CDCL SAT solver.

use contiguous_data::HashSet;
use relational::create_persistent_input;
use relational::database::DatabaseBuilder;

use super::types::{Lit, Var, Weight};
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
    create_persistent_input!(db_builder, _external_inp, external, Lit);
    let mut solver = Solver::new(&mut db_builder, &vars(&[1, 2]), external);
    let mut db = db_builder.build();

    solver.add_clause(&mut db, 0, &[lit(1), lit(2)]); // x1 OR x2
    solver.add_clause(&mut db, 1, &[lit(1), lit(-2)]); // x1 OR NOT x2

    let result = solver.solve(&mut db);
    let assignment = result.expect("expected SAT");
    assert_eq!(assignment.get(&Var::new(1)), Some(&true));
}

#[test]
fn test_simple_unsat() {
    // (x1) AND (NOT x1)
    // UNSAT
    let mut db_builder = DatabaseBuilder::new();
    create_persistent_input!(db_builder, _external_inp, external, Lit);
    let mut solver = Solver::new(&mut db_builder, &vars(&[1]), external);
    let mut db = db_builder.build();

    solver.add_clause(&mut db, 0, &[lit(1)]); // x1
    solver.add_clause(&mut db, 1, &[lit(-1)]); // NOT x1

    assert!(solver.solve(&mut db).is_none());
}

#[test]
fn test_unit_propagation() {
    // (x1) AND (NOT x1 OR x2) AND (NOT x2 OR x3)
    // Unit prop: x1=T -> x2=T -> x3=T
    let mut db_builder = DatabaseBuilder::new();
    create_persistent_input!(db_builder, _external_inp, external, Lit);
    let mut solver = Solver::new(&mut db_builder, &vars(&[1, 2, 3]), external);
    let mut db = db_builder.build();

    solver.add_clause(&mut db, 0, &[lit(1)]); // x1
    solver.add_clause(&mut db, 1, &[lit(-1), lit(2)]); // NOT x1 OR x2
    solver.add_clause(&mut db, 2, &[lit(-2), lit(3)]); // NOT x2 OR x3

    let assignment = solver.solve(&mut db).expect("expected SAT");
    assert_eq!(assignment.get(&Var::new(1)), Some(&true));
    assert_eq!(assignment.get(&Var::new(2)), Some(&true));
    assert_eq!(assignment.get(&Var::new(3)), Some(&true));
}

#[test]
fn test_backtracking() {
    // (x1 OR x2) AND (NOT x1 OR x2) AND (x1 OR NOT x2) AND (NOT x1 OR NOT x2)
    // This is UNSAT (pigeon hole for 2 pigeons, 1 hole)
    let mut db_builder = DatabaseBuilder::new();
    create_persistent_input!(db_builder, _external_inp, external, Lit);
    let mut solver = Solver::new(&mut db_builder, &vars(&[1, 2]), external);
    let mut db = db_builder.build();

    solver.add_clause(&mut db, 0, &[lit(1), lit(2)]); // x1 OR x2
    solver.add_clause(&mut db, 1, &[lit(-1), lit(2)]); // NOT x1 OR x2
    solver.add_clause(&mut db, 2, &[lit(1), lit(-2)]); // x1 OR NOT x2
    solver.add_clause(&mut db, 3, &[lit(-1), lit(-2)]); // NOT x1 OR NOT x2

    assert!(solver.solve(&mut db).is_none());
}

#[test]
fn test_solve_add_clause_solve_again() {
    // First solve: (x1 OR x2) - SAT
    // Then add: (NOT x1) AND (NOT x2) - makes it UNSAT
    let mut db_builder = DatabaseBuilder::new();
    create_persistent_input!(db_builder, _external_inp, external, Lit);
    let mut solver = Solver::new(&mut db_builder, &vars(&[1, 2]), external);
    let mut db = db_builder.build();

    solver.add_clause(&mut db, 0, &[lit(1), lit(2)]); // x1 OR x2

    // First solve - should be SAT
    let result1 = solver.solve(&mut db);
    assert!(result1.is_some(), "first solve should be SAT");

    // Add clauses that make it UNSAT
    solver.add_clause(&mut db, 1, &[lit(-1)]); // NOT x1
    solver.add_clause(&mut db, 2, &[lit(-2)]); // NOT x2

    // Second solve - should be UNSAT
    let result2 = solver.solve(&mut db);
    assert!(result2.is_none(), "second solve should be UNSAT");
}

#[test]
fn test_external_variables() {
    // (x1 OR x2) with external variable x3 = true
    // x3 is not in vars, so it won't be decided, but it participates in propagation
    let mut db_builder = DatabaseBuilder::new();
    create_persistent_input!(db_builder, external_inp, external, Lit);
    // Only x1, x2 are decision variables; x3 is external
    let mut solver = Solver::new(&mut db_builder, &vars(&[1, 2]), external);
    let mut db = db_builder.build();

    // Add clause: (NOT x3 OR x1) - if x3 is true, x1 must be true
    solver.add_clause(&mut db, 0, &[lit(-3), lit(1)]);
    // Add clause: (x1 OR x2)
    solver.add_clause(&mut db, 1, &[lit(1), lit(2)]);

    // Set x3 = true externally
    external_inp.insert(lit(3));
    db.commit();

    // Solve - x3=true should propagate to x1=true
    let assignment = solver.solve(&mut db).expect("expected SAT");
    assert_eq!(
        assignment.get(&Var::new(1)),
        Some(&true),
        "x1 should be true via propagation from x3"
    );
    // x3 should also appear in the assignment
    assert_eq!(
        assignment.get(&Var::new(3)),
        Some(&true),
        "x3 should be in assignment"
    );
}

#[test]
fn test_external_variable_unsat() {
    // Test that external variables can cause UNSAT
    let mut db_builder = DatabaseBuilder::new();
    create_persistent_input!(db_builder, external_inp, external, Lit);
    let mut solver = Solver::new(&mut db_builder, &vars(&[1]), external);
    let mut db = db_builder.build();

    // (x2 OR x1) - at least one must be true
    solver.add_clause(&mut db, 0, &[lit(2), lit(1)]);
    // (NOT x2 OR NOT x1) - at least one must be false
    solver.add_clause(&mut db, 1, &[lit(-2), lit(-1)]);

    // With no external assignment, this is SAT (x1=T, x2=F or x1=F, x2=T)
    let _assignment1 = solver
        .solve(&mut db)
        .expect("expected SAT without external");

    // Now set x2 = false externally
    external_inp.insert(lit(-2));
    db.commit();
    // This forces x1 = true (from clause 0)
    let assignment2 = solver.solve(&mut db).expect("expected SAT with x2=false");
    assert_eq!(
        assignment2.get(&Var::new(1)),
        Some(&true),
        "x1 must be true when x2 is false"
    );
    assert_eq!(assignment2.get(&Var::new(2)), Some(&false));
}

#[test]
fn test_pb_multi_propagation() {
    // PB constraint: a + b + c >= 2 (at least 2 of 3 must be true)
    // If we falsify a, then both b AND c must be true simultaneously
    let mut db_builder = DatabaseBuilder::new();
    create_persistent_input!(db_builder, _external_inp, external, Lit);
    let mut solver = Solver::new(&mut db_builder, &vars(&[1, 2, 3]), external);
    let mut db = db_builder.build();

    // PB constraint: x1 + x2 + x3 >= 2
    let terms: Vec<(Lit, Weight)> = vec![(lit(1), 1), (lit(2), 1), (lit(3), 1)];
    solver.add_pb_constraint(&mut db, 0, &terms, 2);

    // Force x1 = false via a unit clause
    solver.add_clause(&mut db, 1, &[lit(-1)]);

    // Now the PB constraint has slack = 0 + 2 - 2 = 0
    // Both x2 and x3 should be propagated to true
    let assignment = solver.solve(&mut db).expect("expected SAT");
    assert_eq!(
        assignment.get(&Var::new(1)),
        Some(&false),
        "x1 should be false"
    );
    assert_eq!(
        assignment.get(&Var::new(2)),
        Some(&true),
        "x2 should be propagated to true"
    );
    assert_eq!(
        assignment.get(&Var::new(3)),
        Some(&true),
        "x3 should be propagated to true"
    );
}

#[test]
fn test_pb_sequential_propagation_from_same_constraint() {
    // C1: 2a + b + c + d >= 3
    // C2: ¬d (unit clause)
    // C3: ¬b (unit clause)
    //
    // Sequence:
    // 1. C2 propagates ¬d
    // 2. C1: slack = 0 + 4 - 3 = 1; for a with weight 2: 1 < 2, propagate a
    // 3. C3 propagates ¬b
    // 4. C1: slack = 2 + 1 - 3 = 0; for c with weight 1: 0 < 1, propagate c
    //
    // C1 is used twice for propagation!
    let mut db_builder = DatabaseBuilder::new();
    create_persistent_input!(db_builder, _external_inp, external, Lit);
    let mut solver = Solver::new(&mut db_builder, &vars(&[1, 2, 3, 4]), external);
    let mut db = db_builder.build();

    // C1: 2*a + b + c + d >= 3
    // Variables: a=1, b=2, c=3, d=4
    let terms: Vec<(Lit, Weight)> = vec![
        (lit(1), 2), // a with weight 2
        (lit(2), 1), // b with weight 1
        (lit(3), 1), // c with weight 1
        (lit(4), 1), // d with weight 1
    ];
    solver.add_pb_constraint(&mut db, 0, &terms, 3);

    // C2: ¬d (force d = false)
    solver.add_clause(&mut db, 1, &[lit(-4)]);

    // C3: ¬b (force b = false)
    solver.add_clause(&mut db, 2, &[lit(-2)]);

    let assignment = solver.solve(&mut db).expect("expected SAT");

    assert_eq!(
        assignment.get(&Var::new(4)),
        Some(&false),
        "d should be false"
    );
    assert_eq!(
        assignment.get(&Var::new(2)),
        Some(&false),
        "b should be false"
    );
    assert_eq!(
        assignment.get(&Var::new(1)),
        Some(&true),
        "a should be propagated to true by C1 (first use)"
    );
    assert_eq!(
        assignment.get(&Var::new(3)),
        Some(&true),
        "c should be propagated to true by C1 (second use)"
    );
}
