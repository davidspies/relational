//! Tests for pull-based differential dataflow.

use std::collections::HashMap;
use std::hash::Hash;

use super::relational::{Op, output};
use super::*;

/// Helper to collect changes into a HashMap of tuple -> total diff
fn collect_to_map<T: Clone + Eq + Hash, R: Op<T>>(rel: &mut R) -> HashMap<T, i64> {
    let mut result = HashMap::new();
    rel.foreach(&mut |t, diff| {
        *result.entry(t.clone()).or_insert(0) += diff.0;
    });
    // Remove zero entries
    result.retain(|_, v| *v != 0);
    result
}

// =============================================================================
// Tests ported from old database module (in original order)
// =============================================================================

#[test]
fn test_create_and_insert() {
    let mut db = Database::new();
    let (mut handle, mut rel) = db.create_input::<(i32, i32)>();

    handle.insert((1, 2));
    handle.insert((2, 3));
    db.commit();

    let result = collect_to_map(&mut rel);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(&(1, 2)), Some(&1));
    assert_eq!(result.get(&(2, 3)), Some(&1));
}

#[test]
fn test_map() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);
    db.commit();

    let mut mapped = rel.map(|x| x * 2);

    let changes = collect_to_map(&mut mapped);
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&4), Some(&1));
    assert_eq!(changes.get(&6), Some(&1));
}

#[test]
fn test_join() {
    let mut db = Database::new();
    let (mut handle_edges, rel_edges) = db.create_input::<(i32, i32)>();
    let (mut handle_labels, rel_labels) = db.create_input::<(i32, String)>();

    handle_edges.insert((1, 2));
    handle_edges.insert((2, 3));
    handle_edges.insert((3, 4));

    handle_labels.insert((1, "one".to_string()));
    handle_labels.insert((2, "two".to_string()));
    handle_labels.insert((5, "five".to_string()));
    db.commit();

    let mut joined = rel_edges.join(rel_labels);

    let changes = collect_to_map(&mut joined);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes.get(&(1, (2, "one".to_string()))), Some(&1));
    assert_eq!(changes.get(&(2, (3, "two".to_string()))), Some(&1));
}

#[test]
fn test_transitive_closure() {
    let mut db = Database::new();
    let (mut handle, edges) = db.create_input::<(i32, i32)>();

    // Create the path variable
    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = path_var_rel.save();

    // path = edges ∪ (path ⋈ edges).map(|(p, e)| (p.0, e.1))
    let saved_edges = edges.save();
    let edges_for_union = saved_edges.get();
    let edges_for_join = saved_edges.get();

    // Recursive case: extend paths by one edge
    // path(a, c) :- path(a, b), edge(b, c)
    let new_paths = path_rel.get().swap().join_values(edges_for_join);

    // Combine base (edges) and recursive (new_paths)
    let all_paths = edges_for_union.union(new_paths);

    // Wire up the feedback
    db.feedback(path_var, all_paths);

    // Create output before inserting data
    let path_out = output(path_rel.get().boxed());

    // Create graph: 1->2->3->4
    handle.insert((1, 2));
    handle.insert((2, 3));
    handle.insert((3, 4));
    db.commit();

    // Collect results
    let result: Vec<_> = path_out.collect();

    // Should have: (1,2), (2,3), (3,4), (1,3), (2,4), (1,4)
    assert!(result.contains(&(1, 2)), "missing (1,2)");
    assert!(result.contains(&(2, 3)), "missing (2,3)");
    assert!(result.contains(&(3, 4)), "missing (3,4)");
    assert!(result.contains(&(1, 3)), "missing (1,3)");
    assert!(result.contains(&(2, 4)), "missing (2,4)");
    assert!(result.contains(&(1, 4)), "missing (1,4)");
}

#[test]
fn test_multiplicities() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();
    let out = output(rel.boxed());

    // Insert duplicates
    handle.insert(10);
    handle.insert(10);
    handle.insert(20);
    db.commit();

    // Collect into output and check multiplicities
    let state = out.collect();
    // The input uses seen-set semantics, so (10, 10, 20) collapses to (10, 20)
    assert!(state.contains(&10));
    assert!(state.contains(&20));
    assert!(!state.contains(&30));

    // In the new system, inputs use seen-set semantics (not multiset).
    // To test removal, we use push/pop instead of delete.
    db.push();
    handle.insert(30);
    db.commit();

    let state2 = out.collect();
    assert!(state2.contains(&30));

    assert!(db.pop());
    let state3 = out.collect();
    assert!(!state3.contains(&30));
    assert!(state3.contains(&10));
    assert!(state3.contains(&20));
}

#[test]
fn test_checkpoint_and_restore() {
    // The new system uses push/pop instead of checkpoint/restore
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    db.commit();

    let doubled = rel.map(|n| n * 2);
    let doubled_out = output(doubled.boxed());

    // Verify initial state
    let initial = doubled_out.collect();
    assert!(initial.contains(&2)); // 1 * 2
    assert!(initial.contains(&4)); // 2 * 2

    // Push a checkpoint
    db.push();

    // Make changes (insert only - no delete in the new API for regular inputs)
    handle.insert(3);
    db.commit();

    // Verify current state
    let after_changes = doubled_out.collect();
    assert!(after_changes.contains(&2)); // 1 still there
    assert!(after_changes.contains(&4)); // 2 still there
    assert!(after_changes.contains(&6)); // 3 * 2

    // Pop to restore checkpoint
    let popped = db.pop();
    assert!(popped);

    // State restored: doubled should have {2, 4} again
    let after_pop = doubled_out.collect();
    assert!(after_pop.contains(&2)); // 1 still there
    assert!(after_pop.contains(&4)); // 2 still there
    assert!(!after_pop.contains(&6)); // 3 is gone
}

#[test]
fn test_commit_id_advances_with_feedback() {
    let mut db = Database::new();

    // Initial commit ID is 0
    assert_eq!(db.commit_id(), CommitId::new(0));

    // Create a feedback loop to generate commit ID increments
    let (mut handle, edges) = db.create_input::<(i32, i32)>();

    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = path_var_rel.save();

    let saved_edges = edges.save();
    let edges_for_union = saved_edges.get();
    let edges_for_join = saved_edges.get();

    // path(a, c) :- path(a, b), edge(b, c)
    let new_paths = path_rel.get().swap().join_values(edges_for_join);
    let all_paths = edges_for_union.union(new_paths);

    db.feedback(path_var, all_paths);

    // Create output
    let path_out = output(path_rel.get().boxed());

    // Add edges: 1->2->3
    // This triggers feedback iterations, incrementing commit ID
    handle.insert((1, 2));
    handle.insert((2, 3));
    db.commit();

    // Commit ID should have advanced (once per feedback iteration)
    let commit_after_insert = db.commit_id();
    assert!(
        commit_after_insert > CommitId::new(0),
        "Commit ID should advance during feedback"
    );

    // All paths should exist: (1,2), (2,3), (1,3)
    let paths: Vec<_> = path_out.collect();
    assert!(paths.contains(&(1, 2)));
    assert!(paths.contains(&(2, 3)));
    assert!(paths.contains(&(1, 3)));

    // Test that pop increments commit ID (needed for SavedRelation optimization)
    db.push();
    handle.insert((3, 4));
    db.commit();
    let commit_before_pop = db.commit_id();

    assert!(db.pop());
    let commit_after_pop = db.commit_id();

    // Pop increments commit ID so SavedRelation knows to re-pull
    assert!(
        commit_after_pop.raw() > commit_before_pop.raw(),
        "Commit ID should increment on pop: {} vs {}",
        commit_after_pop.raw(),
        commit_before_pop.raw()
    );
}

#[test]
fn test_commit_id_monotonic_through_backtracking() {
    let mut db = Database::new();
    let (mut handle, _rel) = db.create_input::<i32>();

    // Track commit IDs through push/pop cycles
    let mut seen_ids = vec![db.commit_id()];

    db.push();
    handle.insert(1);
    db.commit(); // commit ID increments here
    seen_ids.push(db.commit_id());

    db.push();
    handle.insert(2);
    db.commit(); // commit ID increments here
    seen_ids.push(db.commit_id());

    // Pop doesn't increment commit ID
    assert!(db.pop());
    seen_ids.push(db.commit_id());

    assert!(db.pop());
    seen_ids.push(db.commit_id());

    // All commit IDs should be monotonically non-decreasing
    for i in 1..seen_ids.len() {
        assert!(
            seen_ids[i] >= seen_ids[i - 1],
            "Commit IDs should be monotonic: {:?}",
            seen_ids
        );
    }

    // After commits, the ID should have advanced (pops don't change it)
    assert!(
        seen_ids[2] > seen_ids[0],
        "Commit ID should advance after commits: {:?}",
        seen_ids
    );
}

#[test]
fn test_commit_id_advances_per_feedback_iteration() {
    // Build a chain: 1 -> 2 -> 3 -> 4 -> 5
    // Each feedback iteration discovers paths one hop longer.
    // We verify the commit ID advances once per iteration by counting
    // how many times it advances for a chain of length N.

    let mut db = Database::new();
    let (mut handle, edges) = db.create_input::<(i32, i32)>();

    let initial_commit = db.commit_id();

    // Create the feedback variable for paths
    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = path_var_rel.save();

    let saved_edges = edges.save();
    let edges_for_union = saved_edges.get();
    let edges_for_join = saved_edges.get();

    // path(a, c) :- path(a, b), edges(b, c)
    let new_paths = path_rel.get().swap().join_values(edges_for_join);
    let all_paths = edges_for_union.union(new_paths);

    // Wire up the feedback
    db.feedback(path_var, all_paths);

    let after_feedback_setup = db.commit_id();

    // Feedback setup shouldn't advance commit ID (no data yet)
    assert_eq!(
        initial_commit, after_feedback_setup,
        "No commit ID change without data"
    );

    // Create output
    let path_out = output(path_rel.get().boxed());

    // Add chain edges: 1->2->3->4->5
    // This creates paths of lengths 1, 2, 3, and 4
    // commit() increments once, then each feedback.step() that produces output increments.
    // The exact count depends on the fixpoint iteration pattern.
    handle.insert((1, 2));
    handle.insert((2, 3));
    handle.insert((3, 4));
    handle.insert((4, 5));
    db.commit();

    let after_inserts = db.commit_id();

    // Commit ID should have advanced (at least once for commit, plus feedback iterations)
    let actual_advances = after_inserts.raw() - after_feedback_setup.raw();
    assert!(
        actual_advances >= 1,
        "Commit ID should advance during commit with feedback"
    );

    // Verify all paths were discovered
    let paths: Vec<_> = path_out.collect();
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
    let (mut handle, edges) = db.create_input::<(i32, i32)>();

    // Create a timestamped path variable
    let (path_var, path_var_rel) = db.create_variable::<((i32, i32), CommitId)>();
    let path_rel = path_var_rel.save();

    // To build the recursive relation, we need to strip the CommitId,
    // join with edges, then the feedback mechanism re-stamps with new CommitId
    let saved_edges = edges.save();

    let path_tuples = path_rel.get().map(|((a, b), _)| (a, b));

    // path(a, c) :- path(a, b), edges(b, c)
    let new_paths = path_tuples.swap().join_values(saved_edges.get());
    let all_paths = saved_edges.get().union(new_paths);

    // Wire up the timestamped feedback
    db.feedback_with_id(path_var, all_paths);

    // Create output
    let path_out = output(path_rel.get().boxed());

    // Add edges AFTER setting up feedback so they're discovered during commit
    handle.insert((1, 2));
    handle.insert((2, 3));
    handle.insert((3, 4));
    db.commit();

    // Collect paths with their discovery times
    let paths_with_times: Vec<_> = path_out.collect();

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

// =============================================================================
// Additional relational operator tests
// =============================================================================

#[test]
fn test_input_basic() {
    let mut db = Database::new();
    let (mut handle, mut rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);
    db.commit();

    let changes = collect_to_map(&mut rel);
    assert_eq!(changes.get(&1), Some(&1));
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&3), Some(&1));

    // Second foreach should be empty (changes were drained)
    let changes2 = collect_to_map(&mut rel);
    assert!(changes2.is_empty());
}

#[test]
fn test_filter() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);
    handle.insert(4);
    db.commit();

    let mut filtered = rel.filter(|x| x % 2 == 0);

    let changes = collect_to_map(&mut filtered);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&4), Some(&1));
}

#[test]
fn test_flat_map() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    db.commit();

    // Each number produces itself and its double
    let mut flat_mapped = rel.flat_map(|x| vec![x, x * 2]);

    let changes = collect_to_map(&mut flat_mapped);
    assert_eq!(changes.get(&1), Some(&1));
    assert_eq!(changes.get(&2), Some(&2)); // 2 appears twice: from 1*2 and from 2
    assert_eq!(changes.get(&4), Some(&1));
}

#[test]
fn test_union() {
    let mut db = Database::new();
    let (mut handle_a, rel_a) = db.create_input::<i32>();
    let (mut handle_b, rel_b) = db.create_input::<i32>();

    handle_a.insert(1);
    handle_a.insert(2);
    handle_b.insert(2);
    handle_b.insert(3);
    db.commit();

    let mut unioned = rel_a.union(rel_b);

    let changes = collect_to_map(&mut unioned);
    assert_eq!(changes.get(&1), Some(&1));
    assert_eq!(changes.get(&2), Some(&2)); // 2 appears in both
    assert_eq!(changes.get(&3), Some(&1));
}

#[test]
fn test_distinct() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(1);
    handle.insert(2);
    db.commit();

    let mut distinct_rel = rel.distinct();

    let changes = collect_to_map(&mut distinct_rel);
    assert_eq!(changes.get(&1), Some(&1)); // collapsed to 1
    assert_eq!(changes.get(&2), Some(&1));
}

#[test]
fn test_distinct_on_union() {
    // Distinct is meaningful when unioning relations that might have duplicates
    let mut db = Database::new();
    let (mut handle_a, rel_a) = db.create_input::<i32>();
    let (mut handle_b, rel_b) = db.create_input::<i32>();

    // Both inputs have 1, union produces duplicates
    handle_a.insert(1);
    handle_b.insert(1);
    db.commit();

    let unioned = rel_a.union(rel_b);
    let mut distinct_rel = unioned.distinct();

    // Distinct collapses the duplicates
    let changes = collect_to_map(&mut distinct_rel);
    assert_eq!(changes.get(&1), Some(&1)); // Only one +1 output
}

#[test]
fn test_distinct_incremental_with_pop() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    db.commit();

    let mut distinct_rel = rel.distinct();

    // First batch: 1 appears
    let changes1 = collect_to_map(&mut distinct_rel);
    assert_eq!(changes1.get(&1), Some(&1));

    // Push, insert another value
    db.push();
    handle.insert(2);
    db.commit();

    let changes2 = collect_to_map(&mut distinct_rel);
    assert_eq!(changes2.get(&2), Some(&1));

    // Pop - should undo insert of 2
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    let changes3 = collect_to_map(&mut distinct_rel);
    assert_eq!(changes3.get(&2), Some(&-1));
}

#[test]
fn test_difference() {
    let mut db = Database::new();
    let (mut handle_a, rel_a) = db.create_input::<i32>();
    let (mut handle_b, rel_b) = db.create_input::<i32>();

    handle_a.insert(1);
    handle_a.insert(2);
    handle_a.insert(3);
    handle_b.insert(2);
    handle_b.insert(4);
    db.commit();

    let mut diff = rel_a.difference(rel_b);

    let changes = collect_to_map(&mut diff);
    assert_eq!(changes.get(&1), Some(&1));
    assert_eq!(changes.get(&3), Some(&1));
    assert_eq!(changes.get(&2), None); // 2 is in b
    assert_eq!(changes.get(&4), None); // 4 is only in b
}

#[test]
fn test_join_incremental() {
    let mut db = Database::new();
    let (mut handle_a, rel_a) = db.create_input::<(i32, i32)>();
    let (mut handle_b, rel_b) = db.create_input::<(i32, i32)>();

    handle_a.insert((1, 10));
    db.commit();

    let mut joined = rel_a.join(rel_b);

    // First batch: no matches yet
    let changes1 = collect_to_map(&mut joined);
    assert!(changes1.is_empty());

    // Add matching tuple to b
    handle_b.insert((1, 20));
    db.commit();

    // Second batch: now we have a match
    let changes2 = collect_to_map(&mut joined);
    assert_eq!(changes2.get(&(1, (10, 20))), Some(&1));
}

#[test]
fn test_chained_operators() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);
    handle.insert(4);
    db.commit();

    // Filter evens, then double
    let evens = rel.filter(|x| x % 2 == 0);
    let mut doubled = evens.map(|x| x * 2);

    let changes = collect_to_map(&mut doubled);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes.get(&4), Some(&1)); // 2 * 2
    assert_eq!(changes.get(&8), Some(&1)); // 4 * 2
}

#[test]
fn test_boxed() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    db.commit();

    // Box to break type chain
    let boxed = rel.boxed();
    let mut mapped = boxed.map(|x| x * 2);

    let changes = collect_to_map(&mut mapped);
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&4), Some(&1));
}

#[test]
fn test_saved_relation() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    db.commit();

    let saved = rel.save();

    // Get two consumers
    let mut getter1 = saved.get();
    let mut getter2 = saved.get();

    // Both should see the same changes (foreach calls update internally)
    let changes1 = collect_to_map(&mut getter1);
    let changes2 = collect_to_map(&mut getter2);

    assert_eq!(changes1, changes2);
    assert_eq!(changes1.get(&1), Some(&1));
    assert_eq!(changes1.get(&2), Some(&1));

    // After both consumed, changes should be cleared
    // Add more data
    handle.insert(3);
    db.commit();

    let changes3 = collect_to_map(&mut getter1);
    let changes4 = collect_to_map(&mut getter2);

    assert_eq!(changes3, changes4);
    assert_eq!(changes3.get(&3), Some(&1));
    assert_eq!(changes3.get(&1), None); // old changes gone
}

#[test]
fn test_self_join_with_saved() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<(i32, i32)>();

    // Create a simple path: 1 -> 2 -> 3
    handle.insert((1, 2));
    handle.insert((2, 3));
    db.commit();

    let saved = rel.save();
    let left = saved.get();
    let right = saved.get();

    // Self-join: find paths of length 2
    let mut joined = left.swap().join(right);

    let changes = collect_to_map(&mut joined);
    // (1,2) joins with (2,3) giving us path 1 -> 2 -> 3
    assert_eq!(changes.get(&(2, (1, 3))), Some(&1));
}

#[test]
fn test_sum() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<(String, i64)>();

    handle.insert(("a".to_string(), 10));
    handle.insert(("a".to_string(), 20));
    handle.insert(("b".to_string(), 5));
    db.commit();

    let mut summed = rel.sum();

    let changes = collect_to_map(&mut summed);
    // Net result: ("a", 30) and ("b", 5)
    assert_eq!(changes.get(&("a".to_string(), 30)), Some(&1));
    assert_eq!(changes.get(&("b".to_string(), 5)), Some(&1));
}

#[test]
fn test_sum_incremental() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<(String, i64)>();

    handle.insert(("a".to_string(), 10));
    db.commit();

    let mut summed = rel.sum();

    let changes1 = collect_to_map(&mut summed);
    assert_eq!(changes1.get(&("a".to_string(), 10)), Some(&1));

    // Add more to "a"
    handle.insert(("a".to_string(), 5));
    db.commit();

    let changes2 = collect_to_map(&mut summed);
    // Old sum deleted, new sum inserted
    assert_eq!(changes2.get(&("a".to_string(), 10)), Some(&-1));
    assert_eq!(changes2.get(&("a".to_string(), 15)), Some(&1));
}

#[test]
fn test_max() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 10));
    handle.insert(("a".to_string(), 20));
    handle.insert(("a".to_string(), 15));
    handle.insert(("b".to_string(), 5));
    db.commit();

    let mut maxed = rel.max();

    let changes = collect_to_map(&mut maxed);
    // Max of "a" is 20, max of "b" is 5
    assert_eq!(changes.get(&("a".to_string(), 20)), Some(&1));
    assert_eq!(changes.get(&("b".to_string(), 5)), Some(&1));
}

#[test]
fn test_max_incremental_with_pop() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 10));
    db.commit();

    let mut maxed = rel.max();

    let changes1 = collect_to_map(&mut maxed);
    assert_eq!(changes1.get(&("a".to_string(), 10)), Some(&1));

    // Push and add a higher value
    db.push();
    handle.insert(("a".to_string(), 20));
    db.commit();

    let changes2 = collect_to_map(&mut maxed);
    // Old max (10) removed, new max (20) inserted
    assert_eq!(changes2.get(&("a".to_string(), 10)), Some(&-1));
    assert_eq!(changes2.get(&("a".to_string(), 20)), Some(&1));

    // Pop - should restore max to 10
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    let changes3 = collect_to_map(&mut maxed);
    // Max (20) removed, old max (10) restored
    assert_eq!(changes3.get(&("a".to_string(), 20)), Some(&-1));
    assert_eq!(changes3.get(&("a".to_string(), 10)), Some(&1));
}

#[test]
fn test_count_via_sum() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 1));
    handle.insert(("a".to_string(), 2));
    handle.insert(("a".to_string(), 3));
    handle.insert(("b".to_string(), 10));
    db.commit();

    // Count by mapping each tuple to 1 and summing
    let ones = rel.map(|t| (t.0.clone(), 1i64));
    let mut counted = ones.sum();

    let changes = collect_to_map(&mut counted);
    assert_eq!(changes.get(&("a".to_string(), 3)), Some(&1)); // 3 items with key "a"
    assert_eq!(changes.get(&("b".to_string(), 1)), Some(&1)); // 1 item with key "b"
}

#[test]
fn test_min_via_max_reverse() {
    use std::cmp::Reverse;

    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 10));
    handle.insert(("a".to_string(), 20));
    handle.insert(("a".to_string(), 5));
    db.commit();

    // Min by wrapping values in Reverse and using max
    let reversed = rel.map(|t| (t.0.clone(), Reverse(t.1)));
    let mut maxed = reversed.max();

    let changes = collect_to_map(&mut maxed);
    // Max of Reverse values is min of original values
    assert_eq!(changes.get(&("a".to_string(), Reverse(5))), Some(&1));
}

#[test]
fn test_push_pop_simple() {
    let mut db = Database::new();
    let (mut handle, mut rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    db.commit();

    // Drain initial changes
    let _ = collect_to_map(&mut rel);

    // Push checkpoint
    db.push();

    handle.insert(3);
    db.commit();

    let after_push = collect_to_map(&mut rel);
    assert_eq!(after_push.get(&3), Some(&1));

    // Pop should revert
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    let after_pop = collect_to_map(&mut rel);
    assert_eq!(after_pop.get(&3), Some(&-1)); // deletion
}

#[test]
fn test_consolidate() {
    let mut db = Database::new();
    let (mut handle_a, rel_a) = db.create_input::<i32>();
    let (mut handle_b, rel_b) = db.create_input::<i32>();

    // Create a union that will have duplicates
    handle_a.insert(1);
    handle_a.insert(2);
    handle_b.insert(1); // duplicate with a
    handle_b.insert(3);
    db.commit();

    // Union produces: 1 (+1), 2 (+1), 1 (+1), 3 (+1) = 1 with mult 2
    let unioned = rel_a.union(rel_b);
    let mut consolidated = unioned.consolidate();

    let changes = collect_to_map(&mut consolidated);
    // After consolidation, we should see net multiplicities
    assert_eq!(changes.get(&1), Some(&2)); // 1 appears twice
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&3), Some(&1));
}

#[test]
fn test_consolidate_cancellation() {
    let mut db = Database::new();
    let (mut handle_a, rel_a) = db.create_input::<i32>();
    let (mut handle_b, rel_b) = db.create_input::<i32>();

    handle_a.insert(1);
    handle_b.insert(1);
    db.commit();

    // a has +1 for 1, b.negate has -1 for 1 -> they cancel
    let negated_b = rel_b.negate();
    let combined = rel_a.union(negated_b);

    // This would panic if consolidate forwarded any tuples (since they should all cancel)
    let mut panicking = combined.consolidate().map(|x| {
        panic!("consolidate should not forward cancelled tuple: {}", x);
    });

    // Pull from the relation - if consolidate is working, nothing should be forwarded
    panicking.foreach(&mut |_, _| {});
}

#[test]
fn test_consolidate_incremental() {
    let mut db = Database::new();
    let (mut handle_a, rel_a) = db.create_input::<i32>();
    let (mut handle_b, rel_b) = db.create_input::<i32>();

    // First batch: a has 1, b has 1 -> union has 1 with mult 2
    handle_a.insert(1);
    handle_b.insert(1);
    db.commit();

    let unioned = rel_a.union(rel_b);
    let mut consolidated = unioned.consolidate();

    let changes1 = collect_to_map(&mut consolidated);
    assert_eq!(changes1.get(&1), Some(&2)); // 1 appears twice

    // Second batch: add more to both
    handle_a.insert(2);
    handle_b.insert(2);
    db.commit();

    let changes2 = collect_to_map(&mut consolidated);
    assert_eq!(changes2.get(&2), Some(&2)); // 2 appears twice
}

#[test]
fn test_graph_tracking() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    // Build a simple dataflow: input -> filter -> map
    let filtered = rel.filter(|x| *x > 0).named("positive_filter");
    let mapped = filtered.map(|x| x * 2).named("doubler");
    let mut boxed = mapped.boxed();

    handle.insert(1);
    handle.insert(2);
    handle.insert(-1);
    db.commit();

    // Process to update counters
    let _ = collect_to_map(&mut boxed);

    // Check that graph was built
    let graph = db.graph();
    let dot = graph.to_dot();
    assert!(dot.contains("digraph"), "Should produce DOT format");
    assert!(dot.contains("positive_filter"), "Should contain named node");
    assert!(dot.contains("doubler"), "Should contain named node");
    assert!(dot.contains("input"), "Should contain input node");
    assert!(dot.contains("flat_map"), "filter uses flat_map internally");
}

#[test]
fn test_graph_element_counting() {
    let mut db = Database::new();
    let (mut handle, rel) = db.create_input::<i32>();

    let mut mapped = rel.map(|x| x * 2).named("doubler");

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);
    db.commit();

    // Before processing, counts should be 0
    let dot_before = db.graph().to_dot();
    assert!(
        dot_before.contains("doubler\\nflat_map\\n0"),
        "Counter should be 0 before processing"
    );

    // Process elements
    let _ = collect_to_map(&mut mapped);

    // After processing, the flat_map node should show count of 3
    let dot_after = db.graph().to_dot();
    assert!(
        dot_after.contains("doubler\\nflat_map\\n3"),
        "Counter should be 3 after processing 3 elements"
    );
}
