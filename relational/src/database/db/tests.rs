//! Tests for Database.

use crate::database::{Database, Op, join, map, output, save, union};

/// Test that re-inserting already-present item during push doesn't affect pop.
#[test]
fn test_pop_duplicate_insert() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    let out = output(rel.boxed());

    // Insert 0 before push
    handle.insert(0);
    db.commit();

    assert_eq!(out.collect(), vec![0]);

    // Push
    db.push();

    // Insert 0 again - should be no-op since already in seen set
    handle.insert(0);
    db.commit();

    // Still just 0
    assert_eq!(out.collect(), vec![0]);

    // Pop - should undo nothing since the insert was a no-op
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // 0 should still be present
    assert_eq!(out.collect(), vec![0], "0 should survive pop");
}

/// Test push/pop with transitive closure feedback.
#[test]
fn test_pop_transitive_closure() {
    let mut db = Database::new();
    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();

    // Set up transitive closure: path = edges ∪ (path ⋈ edges)
    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = save(path_var_rel);

    let edges_saved = save(edges_rel);
    let extended = join(path_rel.get(), edges_saved.get(), |(_, b)| *b, |(b, _)| *b);
    let new_paths = map(extended, |((a, _), (_, c))| (a, c));
    let all_paths = union(edges_saved.get(), new_paths);
    db.feedback(path_var, all_paths);

    let path_out = output(path_rel.get().boxed());

    // Push
    db.push();

    // InsertEdge(1, 3)
    edges_h.insert((1, 3));
    db.commit();

    // InsertEdge(3, 1)
    edges_h.insert((3, 1));
    db.commit();

    // Should have paths: (1,3), (3,1), (1,1), (3,3)
    let mut paths: Vec<_> = path_out.collect();
    paths.sort();
    assert_eq!(paths, vec![(1, 1), (1, 3), (3, 1), (3, 3)]);

    // Pop - should undo all edges
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    let result: Vec<_> = path_out.collect();
    // Expected: [] (all edges were added inside pushed frame)
    assert!(result.is_empty(), "Expected empty, got {:?}", result);
}

#[test]
fn test_db_create_input_and_commit() {
    let mut db = Database::new();
    let (mut handle, mut rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);

    // With seen-set semantics, changes are immediately in pending
    // (no staging step). commit() records to checkpoint and runs fixpoint.
    let mut count = 0;
    rel.foreach(&mut |t, diff| {
        assert!(diff.0 > 0);
        assert!(t == 1 || t == 2);
        count += 1;
    });
    assert_eq!(count, 2);

    // After foreach drains pending, commit has nothing new to process
    db.commit();
    let mut count2 = 0;
    rel.foreach(&mut |_, _| count2 += 1);
    assert_eq!(count2, 0); // already drained
}

#[test]
fn test_db_push_pop_simple() {
    let mut db = Database::new();
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
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // Pull the undo changes
    let mut undos = Vec::new();
    rel.foreach(&mut |t, diff| {
        undos.push((t, diff.0));
    });
    undos.sort();
    assert_eq!(undos, vec![(2, -1), (3, -1)]);
}
