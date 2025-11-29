//! Tests for Database2.

use crate::database2::{Database2, Relation};

#[test]
fn test_db_create_input_and_commit() {
    let mut db = Database2::new();
    let (mut handle, mut rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);

    // Before commit, nothing available
    let mut count = 0;
    rel.foreach(&mut |_, _| count += 1);
    assert_eq!(count, 0);

    // After commit, changes are available
    db.commit();
    rel.foreach(&mut |t, diff| {
        assert!(diff.0 > 0);
        assert!(t == 1 || t == 2);
        count += 1;
    });
    assert_eq!(count, 2);
}

#[test]
fn test_db_push_pop_simple() {
    let mut db = Database2::new();
    let (mut handle, mut rel) = db.create_input::<i32>();

    // Initial state
    handle.insert(1);
    db.commit();

    // Drain initial changes
    let mut values = Vec::new();
    rel.foreach(&mut |t, _| values.push(t));
    assert_eq!(values, vec![1]);

    // Push checkpoint
    db.push();

    // Make changes
    handle.insert(2);
    handle.insert(3);
    db.commit();

    // Verify changes are there
    values.clear();
    rel.foreach(&mut |t, _| values.push(t));
    values.sort();
    assert_eq!(values, vec![2, 3]);

    // Pop - should queue undo changes
    db.pop();

    // Pull the undo changes
    let mut undos = Vec::new();
    rel.foreach(&mut |t, diff| {
        undos.push((t, diff.0));
    });
    undos.sort();
    assert_eq!(undos, vec![(2, -1), (3, -1)]);
}
