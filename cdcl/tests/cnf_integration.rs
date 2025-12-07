//! Integration tests for CNF parsing and solving.

use cdcl::{Cnf, Var};

const DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");

fn load_cnf(name: &str) -> Cnf {
    let path = format!("{}/{}", DATA_DIR, name);
    Cnf::from_file(&path).expect("Failed to parse CNF file")
}

#[test]
fn test_simple_sat() {
    let cnf = load_cnf("simple_sat.cnf");
    assert_eq!(cnf.num_vars(), 2);
    assert_eq!(cnf.num_clauses(), 2);

    let result = cnf.solve();
    assert!(result.is_sat());

    // Verify assignment satisfies the formula
    let assignment = result.assignment().unwrap();
    // x1 should be true (either clause requires it when x2 varies)
    assert_eq!(assignment[&Var::new(1)], true);
}

#[test]
fn test_simple_unsat() {
    let cnf = load_cnf("simple_unsat.cnf");
    assert_eq!(cnf.num_vars(), 1);
    assert_eq!(cnf.num_clauses(), 2);

    let result = cnf.solve();
    assert!(result.is_unsat());
}

#[test]
fn test_unit_propagation() {
    let cnf = load_cnf("unit_propagation.cnf");
    assert_eq!(cnf.num_vars(), 3);
    assert_eq!(cnf.num_clauses(), 3);

    let result = cnf.solve();
    assert!(result.is_sat());

    let assignment = result.assignment().unwrap();
    assert_eq!(assignment[&Var::new(1)], true);
    assert_eq!(assignment[&Var::new(2)], true);
    assert_eq!(assignment[&Var::new(3)], true);
}

#[test]
fn test_pigeonhole_unsat() {
    let cnf = load_cnf("pigeonhole_2_1.cnf");
    let result = cnf.solve();
    assert!(result.is_unsat());
}

#[test]
fn test_three_coloring_sat() {
    let cnf = load_cnf("three_coloring.cnf");
    assert_eq!(cnf.num_vars(), 9);
    assert_eq!(cnf.num_clauses(), 21);

    let result = cnf.solve();
    assert!(result.is_sat());

    // Verify the assignment is a valid 3-coloring
    let assignment = result.assignment().unwrap();

    // Each vertex should have exactly one color
    for v in 0..3 {
        let base = v * 3 + 1;
        let colors: Vec<bool> = (0..3)
            .map(|c| {
                assignment
                    .get(&Var::new((base + c) as u32))
                    .copied()
                    .unwrap_or(false)
            })
            .collect();
        let count = colors.iter().filter(|&&c| c).count();
        assert_eq!(count, 1, "Vertex {} should have exactly one color", v + 1);
    }

    // Adjacent vertices should have different colors
    let get_color = |v: usize| -> usize {
        let base = v * 3 + 1;
        (0..3)
            .find(|&c| assignment.get(&Var::new((base + c) as u32)).copied() == Some(true))
            .unwrap()
    };

    let c1 = get_color(0);
    let c2 = get_color(1);
    let c3 = get_color(2);

    assert_ne!(c1, c2, "v1 and v2 should have different colors");
    assert_ne!(c1, c3, "v1 and v3 should have different colors");
    assert_ne!(c2, c3, "v2 and v3 should have different colors");
}

#[test]
fn test_empty_formula() {
    let cnf = load_cnf("empty.cnf");
    assert_eq!(cnf.num_vars(), 0);
    assert_eq!(cnf.num_clauses(), 0);

    let result = cnf.solve();
    assert!(result.is_sat());
}

#[test]
fn test_parse_from_string() {
    let input = "c comment\np cnf 2 1\n1 -2 0\n";
    let cnf = Cnf::parse(input).unwrap();
    assert_eq!(cnf.num_vars(), 2);
    assert_eq!(cnf.num_clauses(), 1);
}

#[test]
fn test_into_solver() {
    let cnf = load_cnf("simple_sat.cnf");
    let (mut db, mut solver, _vars) = cnf.into_solver();
    assert!(solver.solve(&mut db));
}
