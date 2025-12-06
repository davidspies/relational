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

    // Add some glue clauses (LBD <= 2)
    let glue1 = [
        (Lit::pos(Var::new(1)), Level::new(1)),
        (Lit::pos(Var::new(2)), Level::new(2)),
    ];
    let glue2 = [
        (Lit::pos(Var::new(1)), Level::new(1)),
        (Lit::pos(Var::new(2)), Level::new(1)),
    ];
    cd.on_learn(ClauseId::new(1), &glue1);
    cd.on_learn(ClauseId::new(2), &glue2);

    // Add some non-glue clauses (LBD > 2)
    let non_glue = [
        (Lit::pos(Var::new(1)), Level::new(1)),
        (Lit::pos(Var::new(2)), Level::new(2)),
        (Lit::pos(Var::new(3)), Level::new(3)),
    ];
    cd.on_learn(ClauseId::new(3), &non_glue);
    cd.on_learn(ClauseId::new(4), &non_glue);
    cd.on_learn(ClauseId::new(5), &non_glue);
    cd.on_learn(ClauseId::new(6), &non_glue);

    assert!(cd.should_delete());

    let deleted = cd.select_for_deletion();

    // Should delete half of non-glue clauses (4 non-glue -> 2 deleted)
    assert_eq!(deleted.len(), 2);

    // Glue clauses should be protected
    assert!(cd.clause_info.contains_key(&ClauseId::new(1)));
    assert!(cd.clause_info.contains_key(&ClauseId::new(2)));
}
