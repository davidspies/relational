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

use relational::Database;

/// Test: Insert into both sides of a join in a single commit.
/// This exercises the case where left_changes and right_changes are both non-empty.
#[test]
fn test_join_simultaneous_inserts() {
    let mut db = Database::new();

    let left = db.create_input::<(i32, i32)>("left"); // (key, left_val)
    let right = db.create_input::<(i32, i32)>("right"); // (key, right_val)

    // Join on the first element (key)
    let joined = db.join(left, right, |(k, _)| *k, |(k, _)| *k);

    // Initial state: left has (1, 10), right has (1, 100)
    db.insert(left, (1, 10));
    db.insert(right, (1, 100));
    db.commit();

    let result: Vec<_> = db.collect(joined);
    assert_eq!(
        result,
        vec![((1, 10), (1, 100))],
        "Initial join should have one result"
    );

    // Now insert into BOTH sides in a single commit
    // left gets (1, 20) - same key
    // right gets (1, 200) - same key
    db.insert(left, (1, 20));
    db.insert(right, (1, 200));
    db.commit();

    // Expected results:
    // - (1, 10) × (1, 100) = existing
    // - (1, 10) × (1, 200) = old_left × new_right
    // - (1, 20) × (1, 100) = new_left × old_right
    // - (1, 20) × (1, 200) = new_left × new_right  <-- THIS IS THE ONE THAT MIGHT BE MISSING
    let mut result: Vec<_> = db.collect(joined);
    result.sort();

    let mut expected = vec![
        ((1, 10), (1, 100)),
        ((1, 10), (1, 200)),
        ((1, 20), (1, 100)),
        ((1, 20), (1, 200)), // new_left × new_right
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
    let mut db = Database::new();

    let left = db.create_input::<i32>("left");
    let right = db.create_input::<i32>("right");

    // Join where left == right (identity key)
    let joined = db.join(left, right, |x| *x, |x| *x);

    // Insert 1 into both sides in a single commit
    db.insert(left, 1);
    db.insert(right, 1);
    db.commit();

    let result: Vec<_> = db.collect(joined);

    // Should have (1, 1) from left=1 joining with right=1
    assert_eq!(
        result,
        vec![(1, 1)],
        "Join of matching values inserted simultaneously should produce a result"
    );
}

/// Test that verifies multiplicity is correct (not double-counted).
/// This specifically tests the bug where incremental join reads NEW state
/// and computes left_changes × new_right + new_left × right_changes,
/// which double-counts left_changes × right_changes.
#[test]
fn test_join_multiplicity_not_doubled() {
    let mut db = Database::new();

    let left = db.create_input::<i32>("left");
    let right = db.create_input::<i32>("right");

    // Join where left == right (identity key)
    let joined = db.join(left, right, |x| *x, |x| *x);

    // Insert 1 into both sides in a single commit
    db.insert(left, 1);
    db.insert(right, 1);
    db.commit();

    // Check multiplicity - should be exactly 1, not 2
    let multiplicities: Vec<_> = db.iter_with_multiplicity(joined).collect();

    assert_eq!(
        multiplicities.len(),
        1,
        "Should have exactly one distinct tuple"
    );

    let (tuple, mult) = multiplicities[0];
    assert_eq!(*tuple, (1, 1), "Tuple should be (1, 1)");
    assert_eq!(
        mult.0, 1,
        "Multiplicity should be 1, not {} (double-counting bug if 2)",
        mult.0
    );
}

/// Test with multiple matching keys inserted simultaneously.
#[test]
fn test_join_multiple_keys_simultaneous() {
    let mut db = Database::new();

    let left = db.create_input::<(char, i32)>("left"); // (key, val)
    let right = db.create_input::<(char, i32)>("right"); // (key, val)

    let joined = db.join(left, right, |(k, _)| *k, |(k, _)| *k);

    // Insert matching pairs for keys 'a' and 'b' in one commit
    db.insert(left, ('a', 1));
    db.insert(left, ('b', 2));
    db.insert(right, ('a', 10));
    db.insert(right, ('b', 20));
    db.commit();

    let mut result: Vec<_> = db.collect(joined);
    result.sort();

    let mut expected = vec![(('a', 1), ('a', 10)), (('b', 2), ('b', 20))];
    expected.sort();

    assert_eq!(
        result, expected,
        "Simultaneous inserts to both sides should join correctly"
    );
}
