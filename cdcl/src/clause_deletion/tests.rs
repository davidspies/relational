use super::*;
use crate::Var;

#[test]
fn test_compute_lbd() {
    let lits = [
        Lit::pos(Var::new(1)),
        Lit::pos(Var::new(2)),
        Lit::pos(Var::new(3)),
    ];
    let levels = [Level::new(1), Level::new(1), Level::new(2)];
    assert_eq!(compute_lbd(&lits, &levels), 2);

    let levels_same = [Level::new(5), Level::new(5), Level::new(5)];
    assert_eq!(compute_lbd(&lits, &levels_same), 1);

    let levels_diff = [Level::new(1), Level::new(2), Level::new(3)];
    assert_eq!(compute_lbd(&lits, &levels_diff), 3);
}

#[test]
fn test_clause_deletion_glue_protection() {
    let mut cd = ClauseDeletion::new();
    cd.max_clauses = 5; // Low threshold for testing

    let lits = [Lit::pos(Var::new(1)), Lit::pos(Var::new(2))];

    // Add some glue clauses (LBD <= 2)
    cd.on_learn(ClauseId::new(1), &lits, &[Level::new(1), Level::new(2)]);
    cd.on_learn(ClauseId::new(2), &lits, &[Level::new(1), Level::new(1)]);

    // Add some non-glue clauses (LBD > 2)
    let lits3 = [
        Lit::pos(Var::new(1)),
        Lit::pos(Var::new(2)),
        Lit::pos(Var::new(3)),
    ];
    cd.on_learn(
        ClauseId::new(3),
        &lits3,
        &[Level::new(1), Level::new(2), Level::new(3)],
    );
    cd.on_learn(
        ClauseId::new(4),
        &lits3,
        &[Level::new(1), Level::new(2), Level::new(3)],
    );
    cd.on_learn(
        ClauseId::new(5),
        &lits3,
        &[Level::new(1), Level::new(2), Level::new(3)],
    );
    cd.on_learn(
        ClauseId::new(6),
        &lits3,
        &[Level::new(1), Level::new(2), Level::new(3)],
    );

    assert!(cd.should_delete());

    let deleted = cd.select_for_deletion();

    // Should delete half of non-glue clauses (4 non-glue -> 2 deleted)
    assert_eq!(deleted.len(), 2);

    // Glue clauses should be protected
    assert!(cd.clause_info.contains_key(&ClauseId::new(1)));
    assert!(cd.clause_info.contains_key(&ClauseId::new(2)));
}
