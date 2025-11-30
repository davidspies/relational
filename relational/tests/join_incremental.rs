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

use relational::database::{Database, output};

/// Test: Insert into both sides of a join in a single commit.
/// This exercises the case where left_changes and right_changes are both non-empty.
#[test]
fn test_join_simultaneous_inserts() {
    let mut db = Database::new();

    let (mut left, left_rel) = db.create_input::<(i32, i32)>(); // (key, left_val)
    let (mut right, right_rel) = db.create_input::<(i32, i32)>(); // (key, right_val)

    // Join on the first element (key)
    let joined = left_rel.join(right_rel, |(k, _)| *k, |(k, _)| *k);
    let joined_out = output(joined.boxed());

    // Initial state: left has (1, 10), right has (1, 100)
    left.insert((1, 10));
    right.insert((1, 100));
    db.commit();

    let result = joined_out.collect();
    assert_eq!(
        result,
        vec![((1, 10), (1, 100))],
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

    let (mut left, left_rel) = db.create_input::<i32>();
    let (mut right, right_rel) = db.create_input::<i32>();

    // Join where left == right (identity key)
    let joined = left_rel.join(right_rel, |x| *x, |x| *x);
    let joined_out = output(joined.boxed());

    // Insert 1 into both sides in a single commit
    left.insert(1);
    right.insert(1);
    db.commit();

    let result = joined_out.collect();

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

    let (mut left, left_rel) = db.create_input::<i32>();
    let (mut right, right_rel) = db.create_input::<i32>();

    // Join where left == right (identity key)
    let joined = left_rel.join(right_rel, |x| *x, |x| *x);
    let joined_out = output(joined.boxed());

    // Insert 1 into both sides in a single commit
    left.insert(1);
    right.insert(1);
    db.commit();

    // The output's state should have multiplicity exactly 1
    let result = joined_out.collect();

    assert_eq!(result.len(), 1, "Should have exactly one result tuple");
    assert_eq!(result[0], (1, 1), "Tuple should be (1, 1)");
}

/// Test with multiple matching keys inserted simultaneously.
#[test]
fn test_join_multiple_keys_simultaneous() {
    let mut db = Database::new();

    let (mut left, left_rel) = db.create_input::<(char, i32)>(); // (key, val)
    let (mut right, right_rel) = db.create_input::<(char, i32)>(); // (key, val)

    let joined = left_rel.join(right_rel, |(k, _)| *k, |(k, _)| *k);
    let joined_out = output(joined.boxed());

    // Insert matching pairs for keys 'a' and 'b' in one commit
    left.insert(('a', 1));
    left.insert(('b', 2));
    right.insert(('a', 10));
    right.insert(('b', 20));
    db.commit();

    let mut result = joined_out.collect();
    result.sort();

    let mut expected = vec![(('a', 1), ('a', 10)), (('b', 2), ('b', 20))];
    expected.sort();

    assert_eq!(
        result, expected,
        "Simultaneous inserts to both sides should join correctly"
    );
}
