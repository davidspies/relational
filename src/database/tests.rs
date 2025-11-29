//! Unit tests for the database module.

use super::*;
use crate::change::Diff;

#[test]
fn test_create_and_insert() {
    let mut db = Database::new();
    let edges = db.create_input::<(i32, i32)>("edges");

    db.insert(edges, (1, 2));
    db.insert(edges, (2, 3));

    let result: Vec<_> = db.collect(edges);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&(1, 2)));
    assert!(result.contains(&(2, 3)));
}

#[test]
fn test_map() {
    let mut db = Database::new();
    let nums = db.create_input::<i32>("nums");

    db.insert(nums, 1);
    db.insert(nums, 2);
    db.insert(nums, 3);
    db.commit();

    let doubled = db.map(nums, |x| x * 2);
    db.commit();

    let result: Vec<_> = db.collect(doubled);
    assert!(result.contains(&2));
    assert!(result.contains(&4));
    assert!(result.contains(&6));
}

#[test]
fn test_join() {
    let mut db = Database::new();
    let edges = db.create_input::<(i32, i32)>("edges");
    let labels = db.create_input::<(i32, &str)>("labels");

    db.insert(edges, (1, 2));
    db.insert(edges, (2, 3));
    db.insert(labels, (1, "one"));
    db.insert(labels, (2, "two"));
    db.commit();

    // Join edges with labels on the source node
    let joined = db.join(edges, labels, |(src, _)| *src, |(id, _)| *id);
    db.commit();

    let result: Vec<_> = db.collect(joined);
    assert_eq!(result.len(), 2);
}

#[test]
fn test_transitive_closure() {
    let mut db = Database::new();
    let edges = db.create_input::<(i32, i32)>("edges");

    // Create graph: 1->2->3->4
    db.insert(edges, (1, 2));
    db.insert(edges, (2, 3));
    db.insert(edges, (3, 4));
    db.commit();

    // path = edges ∪ (path ⋈ edges).map(|(p, e)| (p.0, e.1))
    let (path_var, path) = db.variable::<(i32, i32)>("path");

    // Recursive case: extend paths by one edge
    // path(a, c) :- path(a, b), edge(b, c)
    let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
    let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));

    // Combine base (edges) and recursive (new_paths)
    let all_paths = db.union(edges, new_paths);

    db.feedback(path_var, edges, all_paths);

    let result: Vec<_> = db.collect(path);
    // Should have: (1,2), (2,3), (3,4), (1,3), (2,4), (1,4)
    assert!(result.contains(&(1, 2)));
    assert!(result.contains(&(2, 3)));
    assert!(result.contains(&(3, 4)));
    assert!(result.contains(&(1, 3)));
    assert!(result.contains(&(2, 4)));
    assert!(result.contains(&(1, 4)));
}

#[test]
fn test_multiplicities() {
    let mut db = Database::new();
    let items = db.create_input::<i32>("items");

    // Insert duplicates
    db.insert(items, 10);
    db.insert(items, 10);
    db.insert(items, 20);

    // Check multiplicities
    assert_eq!(db.multiplicity(items, &10), Diff(2));
    assert_eq!(db.multiplicity(items, &20), Diff(1));
    assert_eq!(db.multiplicity(items, &30), Diff(0));

    // Delete one occurrence
    db.delete(items, 10);
    assert_eq!(db.multiplicity(items, &10), Diff(1));

    // iter_with_multiplicity shows all tuples
    let mults: Vec<_> = db.iter_with_multiplicity(items).collect();
    assert_eq!(mults.len(), 2); // 10 and 20
}

#[test]
fn test_checkpoint_and_restore() {
    let mut db = Database::new();
    let numbers = db.create_input::<i32>("numbers");
    db.insert(numbers, 1);
    db.insert(numbers, 2);
    db.commit();

    let doubled = db.map(numbers, |n| n * 2);
    db.commit();

    // Create checkpoint
    let cp = db.checkpoint(Some("initial"));

    // Verify checkpoint is listed
    let checkpoints = db.list_checkpoints();
    assert_eq!(checkpoints.len(), 1);
    assert_eq!(checkpoints[0].1, Some("initial"));

    // Make changes
    db.insert(numbers, 3);
    db.delete(numbers, 1);
    db.commit();

    // Verify current state
    assert_eq!(db.collect(numbers).len(), 2); // {2, 3}
    let doubled_result: Vec<_> = db.collect(doubled);
    assert!(doubled_result.contains(&4));
    assert!(doubled_result.contains(&6));

    // Restore checkpoint
    let info = db.restore(cp).unwrap();

    // Derived relations are restored
    assert!(!info.restored_nodes.is_empty());

    // Manual inputs are NOT auto-restored (numbers is manual input)
    assert!(!info.manual_input_nodes.is_empty());

    // The 'doubled' relation was restored to checkpoint state (2, 4)
    // But since numbers was NOT restored, the states may be inconsistent
    // until we propagate changes or fix inputs manually
}

#[test]
fn test_commit_id_advances_with_feedback() {
    let mut db = Database::new();

    // Initial commit ID is 0
    assert_eq!(db.commit_id(), CommitId(0));

    // Create a feedback loop to generate commit ID increments
    let edges = db.create_input::<(i32, i32)>("edges");
    let (path_var, path) = db.variable::<(i32, i32)>("path");

    // path(a, c) :- path(a, b), edge(b, c)
    let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
    let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
    let all_paths = db.union(edges, new_paths);

    db.feedback(path_var, edges, all_paths);

    // Add edges: 1->2->3
    // This triggers feedback iterations, incrementing commit ID
    db.insert(edges, (1, 2));
    db.insert(edges, (2, 3));
    db.commit();

    // Commit ID should have advanced (once per feedback iteration)
    let commit_after_insert = db.commit_id();
    assert!(
        commit_after_insert > CommitId(0),
        "Commit ID should advance during feedback"
    );

    // All paths should exist: (1,2), (2,3), (1,3)
    let paths: Vec<_> = db.collect(path);
    assert!(paths.contains(&(1, 2)));
    assert!(paths.contains(&(2, 3)));
    assert!(paths.contains(&(1, 3)));

    // Test that pop increments commit ID
    db.push(None);
    db.insert(edges, (3, 4));
    db.commit();
    let commit_before_pop = db.commit_id();

    db.pop();
    let commit_after_pop = db.commit_id();

    assert!(
        commit_after_pop > commit_before_pop,
        "Commit ID should advance on pop: {} vs {}",
        commit_after_pop.raw(),
        commit_before_pop.raw()
    );
}

#[test]
fn test_commit_id_monotonic_through_backtracking() {
    let mut db = Database::new();
    let items = db.create_input::<i32>("items");

    // Track commit IDs through push/pop cycles
    let mut seen_ids = vec![db.commit_id()];

    db.push(None);
    db.insert(items, 1);
    db.commit();
    seen_ids.push(db.commit_id());

    db.push(None);
    db.insert(items, 2);
    db.commit();
    seen_ids.push(db.commit_id());

    // Pop should increment commit ID
    db.pop();
    seen_ids.push(db.commit_id());

    db.pop();
    seen_ids.push(db.commit_id());

    // All commit IDs should be monotonically non-decreasing
    for i in 1..seen_ids.len() {
        assert!(
            seen_ids[i] >= seen_ids[i - 1],
            "Commit IDs should be monotonic: {:?}",
            seen_ids
        );
    }

    // After pops, the ID should have advanced
    assert!(
        seen_ids.last().unwrap() > seen_ids.first().unwrap(),
        "Final commit ID should be greater than initial"
    );
}

#[test]
fn test_commit_id_advances_per_feedback_iteration() {
    // Build a chain: 1 -> 2 -> 3 -> 4 -> 5
    // Each feedback iteration discovers paths one hop longer.
    // We verify the commit ID advances once per iteration by counting
    // how many times it advances for a chain of length N.

    let mut db = Database::new();
    let edges = db.create_input::<(i32, i32)>("edges");

    let initial_commit = db.commit_id();

    // Create the feedback variable for paths
    let (path_var, path) = db.variable::<(i32, i32)>("path");

    // path(a, c) :- path(a, b), edges(b, c)
    let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
    let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
    let all_paths = db.union(edges, new_paths);

    // Wire up the feedback
    db.feedback(path_var, edges, all_paths);

    let after_feedback_setup = db.commit_id();

    // Feedback setup shouldn't advance commit ID (no data yet)
    assert_eq!(
        initial_commit, after_feedback_setup,
        "No commit ID change without data"
    );

    // Add chain edges: 1->2->3->4->5
    // This creates paths of lengths 1, 2, 3, and 4
    // Iteration 1: discover (1,2), (2,3), (3,4), (4,5) - length 1
    // Iteration 2: discover (1,3), (2,4), (3,5) - length 2
    // Iteration 3: discover (1,4), (2,5) - length 3
    // Iteration 4: discover (1,5) - length 4
    // That's 4 feedback iterations = 4 commit ID increments
    db.insert(edges, (1, 2));
    db.insert(edges, (2, 3));
    db.insert(edges, (3, 4));
    db.insert(edges, (4, 5));
    db.commit();

    let after_inserts = db.commit_id();

    // We should have advanced exactly 4 times (once per path length)
    let expected_advances = 4u64;
    let actual_advances = after_inserts.raw() - after_feedback_setup.raw();

    assert_eq!(
        actual_advances, expected_advances,
        "Expected {} commit ID advances for chain of length 4, got {}",
        expected_advances, actual_advances
    );

    // Verify all paths were discovered
    let paths: Vec<_> = db.collect(path);
    assert_eq!(paths.len(), 10); // 4 + 3 + 2 + 1 paths

    // Check specific paths exist
    assert!(paths.contains(&(1, 5)), "Should have path 1->5");
    assert!(paths.contains(&(1, 4)), "Should have path 1->4");
    assert!(paths.contains(&(2, 5)), "Should have path 2->5");
}

#[test]
fn test_feedback_with_id_discovery_order() {
    // Test that feedback_with_id correctly tracks when tuples are discovered.
    // Longer paths should have higher commit IDs than shorter paths.
    let mut db = Database::new();
    let edges = db.create_input::<(i32, i32)>("edges");

    // Add all edges BEFORE setting up feedback, so they're all discovered together
    db.insert(edges, (1, 2));
    db.insert(edges, (2, 3));
    db.insert(edges, (3, 4));
    db.commit();

    // Create a timestamped path variable
    let (path_var, path) = db.variable::<((i32, i32), CommitId)>("path");

    // To build the recursive relation, we need to strip the CommitId,
    // join with edges, then the feedback mechanism re-stamps with new CommitId
    let path_tuples = db.map(path, |((a, b), _)| (*a, *b));

    // path(a, c) :- path(a, b), edges(b, c)
    let extended = db.join(path_tuples, edges, |(_, b)| *b, |(b, _)| *b);
    let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
    let all_paths = db.union(edges, new_paths);

    // Wire up the timestamped feedback - this runs fixpoint and discovers all paths
    db.feedback_with_id(path_var, edges, all_paths);

    // Collect paths with their discovery times
    let paths_with_times: Vec<_> = db.collect(path);

    // Extract commit IDs for paths of different lengths
    let get_commit_id = |from: i32, to: i32| -> Option<CommitId> {
        paths_with_times
            .iter()
            .find(|((a, b), _)| *a == from && *b == to)
            .map(|(_, id)| *id)
    };

    // Length 1 paths: (1,2), (2,3), (3,4)
    let id_1_2 = get_commit_id(1, 2).expect("Should have path 1->2");
    let id_2_3 = get_commit_id(2, 3).expect("Should have path 2->3");
    let id_3_4 = get_commit_id(3, 4).expect("Should have path 3->4");

    // Length 2 paths: (1,3), (2,4)
    let id_1_3 = get_commit_id(1, 3).expect("Should have path 1->3");
    let id_2_4 = get_commit_id(2, 4).expect("Should have path 2->4");

    // Length 3 path: (1,4)
    let id_1_4 = get_commit_id(1, 4).expect("Should have path 1->4");

    // All length-1 paths should have the same commit ID (discovered in same iteration)
    assert_eq!(id_1_2, id_2_3, "Length-1 paths should have same commit ID");
    assert_eq!(id_2_3, id_3_4, "Length-1 paths should have same commit ID");

    // Length-2 paths should have higher commit ID than length-1
    assert!(
        id_1_3 > id_1_2,
        "Length-2 path should be discovered after length-1: {:?} vs {:?}",
        id_1_3,
        id_1_2
    );
    assert_eq!(id_1_3, id_2_4, "Length-2 paths should have same commit ID");

    // Length-3 path should have higher commit ID than length-2
    assert!(
        id_1_4 > id_1_3,
        "Length-3 path should be discovered after length-2: {:?} vs {:?}",
        id_1_4,
        id_1_3
    );
}
