use super::*;
use crate::Var;

#[test]
fn test_compute_lbd() {
    let clause = [
        (Lit::pos(Var::new(1)), Level::new(1)),
        (Lit::pos(Var::new(2)), Level::new(1)),
        (Lit::pos(Var::new(3)), Level::new(2)),
    ];
    assert_eq!(compute_lbd(&clause), 2);

    let clause_same = [
        (Lit::pos(Var::new(1)), Level::new(5)),
        (Lit::pos(Var::new(2)), Level::new(5)),
        (Lit::pos(Var::new(3)), Level::new(5)),
    ];
    assert_eq!(compute_lbd(&clause_same), 1);

    let clause_diff = [
        (Lit::pos(Var::new(1)), Level::new(1)),
        (Lit::pos(Var::new(2)), Level::new(2)),
        (Lit::pos(Var::new(3)), Level::new(3)),
    ];
    assert_eq!(compute_lbd(&clause_diff), 3);
}

#[test]
fn test_clause_deletion_glue_protection() {
    let mut cd = ClauseDeletion::new();
    cd.max_clauses = 5; // Low threshold for testing

    // Add some glue constraints (LBD <= 2)
    let glue1 = [
        (Lit::pos(Var::new(1)), Level::new(1)),
        (Lit::pos(Var::new(2)), Level::new(2)),
    ];
    let glue2 = [
        (Lit::pos(Var::new(1)), Level::new(1)),
        (Lit::pos(Var::new(2)), Level::new(1)),
    ];
    cd.on_learn(ConstraintId::Learned(0), &glue1);
    cd.on_learn(ConstraintId::Learned(1), &glue2);

    // Add some non-glue constraints (LBD > 2)
    let non_glue = [
        (Lit::pos(Var::new(1)), Level::new(1)),
        (Lit::pos(Var::new(2)), Level::new(2)),
        (Lit::pos(Var::new(3)), Level::new(3)),
    ];
    cd.on_learn(ConstraintId::Learned(2), &non_glue);
    cd.on_learn(ConstraintId::Learned(3), &non_glue);
    cd.on_learn(ConstraintId::Learned(4), &non_glue);
    cd.on_learn(ConstraintId::Learned(5), &non_glue);

    assert!(cd.should_delete());

    let deleted = cd.select_for_deletion();

    // Should delete half of non-glue constraints (4 non-glue -> 2 deleted)
    assert_eq!(deleted.len(), 2);

    // Glue constraints should be protected
    assert!(cd.constraint_info.contains_key(&ConstraintId::Learned(0)));
    assert!(cd.constraint_info.contains_key(&ConstraintId::Learned(1)));
}
