//! Integration tests for OPB parsing and solving.

use cdcl::{Opb, Var};

const DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/opb_data");

fn load_opb(name: &str) -> Opb {
    let path = format!("{}/{}", DATA_DIR, name);
    Opb::from_file(&path).expect("Failed to parse OPB file")
}

#[test]
fn test_simple_sat() {
    let opb = load_opb("simple_sat.opb");
    assert_eq!(opb.num_vars(), 2);
    assert_eq!(opb.num_constraints(), 2);

    let result = opb.solve();
    assert!(result.is_sat());

    let assignment = result.assignment().unwrap();
    assert!(assignment[&Var::new(1)]);
}

#[test]
fn test_pigeonhole_unsat() {
    // 3 pigeons, 2 holes - impossible
    let opb = load_opb("pigeonhole_3_2.opb");
    let result = opb.solve();
    assert!(result.is_unsat());
}

#[test]
fn test_cardinality_sat() {
    let opb = load_opb("cardinality_sat.opb");
    assert_eq!(opb.num_vars(), 4);
    assert_eq!(opb.num_constraints(), 2);

    let result = opb.solve();
    assert!(result.is_sat());

    // Verify: at least 2 true, at most 3 true
    let assignment = result.assignment().unwrap();
    let true_count: usize = (1..=4)
        .filter(|&i| assignment.get(&Var::new(i)).copied().unwrap_or(false))
        .count();
    assert!(true_count >= 2, "At least 2 must be true");
    assert!(true_count <= 3, "At most 3 can be true");
}

#[test]
fn test_weighted_sat() {
    let opb = load_opb("weighted_sat.opb");
    assert_eq!(opb.num_vars(), 4);
    assert_eq!(opb.num_constraints(), 1);

    let result = opb.solve();
    assert!(result.is_sat());

    // Verify weighted sum >= 4
    let assignment = result.assignment().unwrap();
    let weights = [3, 2, 1, 1];
    let weighted_sum: i64 = (1..=4)
        .map(|i| {
            if assignment.get(&Var::new(i)).copied().unwrap_or(false) {
                weights[i as usize - 1]
            } else {
                0
            }
        })
        .sum();
    assert!(weighted_sum >= 4, "Weighted sum must be >= 4, got {}", weighted_sum);
}

#[test]
fn test_parse_from_string() {
    let input = "* comment\n* #variable= 2 #constraint= 1\n+1 x1 +1 x2 >= 1 ;\n";
    let opb = Opb::parse(input).unwrap();
    assert_eq!(opb.num_vars(), 2);
    assert_eq!(opb.num_constraints(), 1);
}

#[test]
fn test_into_solver() {
    let opb = load_opb("simple_sat.opb");
    let (mut db, mut solver, _vars) = opb.into_solver();
    assert!(solver.solve(&mut db).is_some());
}
