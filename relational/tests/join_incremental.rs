//! Test that join handles simultaneous changes to both inputs correctly.
//!
//! When both inputs to a join receive changes in the same commit, the incremental
//! computation must produce: (new_A × old_B) ∪ (new_A × new_B) ∪ (old_A × new_B)
//! which simplifies to: (new_A × old_B) ∪ (new_A_total × new_B)
//! where new_A_total = old_A ∪ new_A
//!
//! BUG: If incremental function reads NEW input state and computes:
//!   left_changes × new_right + new_left × right_changes
//! This double-counts (left_changes × right_changes).

use relational::database::DatabaseBuilder;

/// Test: Insert into both sides of a join in a single commit.
/// This exercises the case where left_changes and right_changes are both non-empty.
#[test]
fn test_join_simultaneous_inserts() {
    let mut db = DatabaseBuilder::new();

    let (left, left_rel) = db.create_input::<(i32, i32)>(); // (key, left_val)
    let (right, right_rel) = db.create_input::<(i32, i32)>(); // (key, right_val)

    // Join on the first element (key)
    let joined = left_rel.join(right_rel);
    let joined_out = joined.boxed().output();

    // Initial state: left has (1, 10), right has (1, 100)
    left.insert((1, 10));
    right.insert((1, 100));
    let mut db = db.build();
    db.commit();

    let result = joined_out.collect();
    assert_eq!(
        result,
        vec![(1, (10, 100))],
        "Initial join should have one result"
    );

    // Now insert into BOTH sides in a single commit
    // left gets (1, 20) - same key
    // right gets (1, 200) - same key
    left.insert((1, 20));
    right.insert((1, 200));
    db.commit();

    // Expected results:
    // - (1, 10) × (1, 100) = existing
    // - (1, 10) × (1, 200) = old_left × new_right
    // - (1, 20) × (1, 100) = new_left × old_right
    // - (1, 20) × (1, 200) = new_left × new_right  <-- THIS IS THE ONE THAT MIGHT BE MISSING
    let mut result = joined_out.collect();
    result.sort();

    let mut expected = vec![
        (1, (10, 100)),
        (1, (10, 200)),
        (1, (20, 100)),
        (1, (20, 200)), // new_left × new_right
    ];
    expected.sort();

    assert_eq!(
        result, expected,
        "Join should include new_left × new_right pair"
    );
}

/// Simpler test: empty initial state, insert into both sides at once.
#[test]
fn test_join_both_sides_from_empty() {
    let mut db = DatabaseBuilder::new();

    let (left, left_rel) = db.create_input::<(i32, ())>();
    let (right, right_rel) = db.create_input::<(i32, ())>();

    // Join where left == right (identity key)
    let joined = left_rel.join(right_rel);
    let joined_out = joined.boxed().output();

    // Insert 1 into both sides in a single commit
    left.insert((1, ()));
    right.insert((1, ()));
    let mut db = db.build();
    db.commit();

    let result = joined_out.collect();

    // Should have (1, ((), ())) from left=1 joining with right=1
    assert_eq!(
        result,
        vec![(1, ((), ()))],
        "Join of matching values inserted simultaneously should produce a result"
    );
}

/// Test that verifies multiplicity is correct (not double-counted).
/// This specifically tests the bug where incremental join reads NEW state
/// and computes left_changes × new_right + new_left × right_changes,
/// which double-counts left_changes × right_changes.
#[test]
fn test_join_multiplicity_not_doubled() {
    let mut db = DatabaseBuilder::new();

    let (left, left_rel) = db.create_input::<(i32, ())>();
    let (right, right_rel) = db.create_input::<(i32, ())>();

    // Join where left == right (identity key)
    let joined = left_rel.join(right_rel);
    let joined_out = joined.boxed().output();

    // Insert 1 into both sides in a single commit
    left.insert((1, ()));
    right.insert((1, ()));
    let mut db = db.build();
    db.commit();

    // The output's state should have multiplicity exactly 1
    let result = joined_out.collect();

    assert_eq!(result.len(), 1, "Should have exactly one result tuple");
    assert_eq!(result[0], (1, ((), ())), "Tuple should be (1, ((), ()))");
}

/// Test with multiple matching keys inserted simultaneously.
#[test]
fn test_join_multiple_keys_simultaneous() {
    let mut db = DatabaseBuilder::new();

    let (left, left_rel) = db.create_input::<(char, i32)>(); // (key, val)
    let (right, right_rel) = db.create_input::<(char, i32)>(); // (key, val)

    let joined = left_rel.join(right_rel);
    let joined_out = joined.boxed().output();

    // Insert matching pairs for keys 'a' and 'b' in one commit
    left.insert(('a', 1));
    left.insert(('b', 2));
    right.insert(('a', 10));
    right.insert(('b', 20));
    let mut db = db.build();
    db.commit();

    let mut result = joined_out.collect();
    result.sort();

    let mut expected = vec![('a', (1, 10)), ('b', (2, 20))];
    expected.sort();

    assert_eq!(
        result, expected,
        "Simultaneous inserts to both sides should join correctly"
    );
}
