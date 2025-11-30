//! Tests for pull-based differential dataflow.

use std::collections::HashMap;

use super::*;

/// Helper to collect changes into a HashMap of tuple -> total diff
fn collect_to_map<T: crate::Tuple, R: Relation<T>>(rel: &mut R) -> HashMap<T, i64> {
    let mut result = HashMap::new();
    rel.foreach(&mut |t, diff| {
        *result.entry(t.clone()).or_insert(0) += diff.0;
    });
    // Remove zero entries
    result.retain(|_, v| *v != 0);
    result
}

#[test]
fn test_input_basic() {
    let mut db = Database2::new();
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
fn test_map() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);
    db.commit();

    let mut mapped = map(rel, |x| x * 2);

    let changes = collect_to_map(&mut mapped);
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&4), Some(&1));
    assert_eq!(changes.get(&6), Some(&1));
}

#[test]
fn test_filter() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);
    handle.insert(4);
    db.commit();

    let mut filtered = filter(rel, |x| x % 2 == 0);

    let changes = collect_to_map(&mut filtered);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&4), Some(&1));
}

#[test]
fn test_flat_map() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    db.commit();

    // Each number produces itself and its double
    let mut flat_mapped = flat_map(rel, |x| vec![x, x * 2]);

    let changes = collect_to_map(&mut flat_mapped);
    assert_eq!(changes.get(&1), Some(&1));
    assert_eq!(changes.get(&2), Some(&2)); // 2 appears twice: from 1*2 and from 2
    assert_eq!(changes.get(&4), Some(&1));
}

#[test]
fn test_union() {
    let mut db = Database2::new();
    let (mut handle_a, rel_a) = db.create_input::<i32>();
    let (mut handle_b, rel_b) = db.create_input::<i32>();

    handle_a.insert(1);
    handle_a.insert(2);
    handle_b.insert(2);
    handle_b.insert(3);
    db.commit();

    let mut unioned = union(rel_a, rel_b);

    let changes = collect_to_map(&mut unioned);
    assert_eq!(changes.get(&1), Some(&1));
    assert_eq!(changes.get(&2), Some(&2)); // 2 appears in both
    assert_eq!(changes.get(&3), Some(&1));
}

#[test]
fn test_distinct() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(1);
    handle.insert(2);
    db.commit();

    let mut distinct_rel = distinct(rel);

    let changes = collect_to_map(&mut distinct_rel);
    assert_eq!(changes.get(&1), Some(&1)); // collapsed to 1
    assert_eq!(changes.get(&2), Some(&1));
}

#[test]
fn test_distinct_on_union() {
    // Distinct is meaningful when unioning relations that might have duplicates
    let mut db = Database2::new();
    let (mut handle_a, rel_a) = db.create_input::<i32>();
    let (mut handle_b, rel_b) = db.create_input::<i32>();

    // Both inputs have 1, union produces duplicates
    handle_a.insert(1);
    handle_b.insert(1);
    db.commit();

    let unioned = union(rel_a, rel_b);
    let mut distinct_rel = distinct(unioned);

    // Distinct collapses the duplicates
    let changes = collect_to_map(&mut distinct_rel);
    assert_eq!(changes.get(&1), Some(&1)); // Only one +1 output
}

#[test]
fn test_distinct_incremental_with_pop() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    db.commit();

    let mut distinct_rel = distinct(rel);

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
    db.pop();

    let changes3 = collect_to_map(&mut distinct_rel);
    assert_eq!(changes3.get(&2), Some(&-1));
}

#[test]
fn test_difference() {
    let mut db = Database2::new();
    let (mut handle_a, rel_a) = db.create_input::<i32>();
    let (mut handle_b, rel_b) = db.create_input::<i32>();

    handle_a.insert(1);
    handle_a.insert(2);
    handle_a.insert(3);
    handle_b.insert(2);
    handle_b.insert(4);
    db.commit();

    let mut diff = difference(rel_a, rel_b);

    let changes = collect_to_map(&mut diff);
    assert_eq!(changes.get(&1), Some(&1));
    assert_eq!(changes.get(&3), Some(&1));
    assert_eq!(changes.get(&2), None); // 2 is in b
    assert_eq!(changes.get(&4), None); // 4 is only in b
}

#[test]
fn test_join() {
    let mut db = Database2::new();
    let (mut handle_edges, rel_edges) = db.create_input::<(i32, i32)>();
    let (mut handle_labels, rel_labels) = db.create_input::<(i32, String)>();

    handle_edges.insert((1, 2));
    handle_edges.insert((2, 3));
    handle_edges.insert((3, 4));

    handle_labels.insert((1, "one".to_string()));
    handle_labels.insert((2, "two".to_string()));
    handle_labels.insert((5, "five".to_string()));
    db.commit();

    let mut joined = join(rel_edges, rel_labels, |e| e.0, |l| l.0);

    let changes = collect_to_map(&mut joined);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes.get(&((1, 2), (1, "one".to_string()))), Some(&1));
    assert_eq!(changes.get(&((2, 3), (2, "two".to_string()))), Some(&1));
}

#[test]
fn test_join_incremental() {
    let mut db = Database2::new();
    let (mut handle_a, rel_a) = db.create_input::<(i32, i32)>();
    let (mut handle_b, rel_b) = db.create_input::<(i32, i32)>();

    handle_a.insert((1, 10));
    db.commit();

    let mut joined = join(rel_a, rel_b, |a| a.0, |b| b.0);

    // First batch: no matches yet
    let changes1 = collect_to_map(&mut joined);
    assert!(changes1.is_empty());

    // Add matching tuple to b
    handle_b.insert((1, 20));
    db.commit();

    // Second batch: now we have a match
    let changes2 = collect_to_map(&mut joined);
    assert_eq!(changes2.get(&((1, 10), (1, 20))), Some(&1));
}

#[test]
fn test_chained_operators() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);
    handle.insert(4);
    db.commit();

    // Filter evens, then double
    let evens = filter(rel, |x| x % 2 == 0);
    let mut doubled = map(evens, |x| x * 2);

    let changes = collect_to_map(&mut doubled);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes.get(&4), Some(&1)); // 2 * 2
    assert_eq!(changes.get(&8), Some(&1)); // 4 * 2
}

#[test]
fn test_boxed() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    db.commit();

    // Box to break type chain
    let boxed = rel.boxed();
    let mut mapped = map(boxed, |x| x * 2);

    let changes = collect_to_map(&mut mapped);
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&4), Some(&1));
}

#[test]
fn test_saved_relation() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    db.commit();

    let mut saved = save(rel);

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
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<(i32, i32)>();

    // Create a simple path: 1 -> 2 -> 3
    handle.insert((1, 2));
    handle.insert((2, 3));
    db.commit();

    let mut saved = save(rel);
    let left = saved.get();
    let right = saved.get();

    // Self-join: find paths of length 2
    let mut joined = join(left, right, |e| e.1, |e| e.0);

    let changes = collect_to_map(&mut joined);
    // (1,2) joins with (2,3) giving us path 1 -> 2 -> 3
    assert_eq!(changes.get(&((1, 2), (2, 3))), Some(&1));
}

#[test]
fn test_sum() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<(String, i64)>();

    handle.insert(("a".to_string(), 10));
    handle.insert(("a".to_string(), 20));
    handle.insert(("b".to_string(), 5));
    db.commit();

    let mut summed = sum(rel, |t| t.0.clone(), |t| t.1);

    let changes = collect_to_map(&mut summed);
    // Net result: ("a", 30) and ("b", 5)
    assert_eq!(changes.get(&("a".to_string(), 30)), Some(&1));
    assert_eq!(changes.get(&("b".to_string(), 5)), Some(&1));
}

#[test]
fn test_sum_incremental() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<(String, i64)>();

    handle.insert(("a".to_string(), 10));
    db.commit();

    let mut summed = sum(rel, |t| t.0.clone(), |t| t.1);

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
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 10));
    handle.insert(("a".to_string(), 20));
    handle.insert(("a".to_string(), 15));
    handle.insert(("b".to_string(), 5));
    db.commit();

    let mut maxed = max(rel, |t| t.0.clone(), |t| t.1);

    let changes = collect_to_map(&mut maxed);
    // Max of "a" is 20, max of "b" is 5
    assert_eq!(changes.get(&("a".to_string(), 20)), Some(&1));
    assert_eq!(changes.get(&("b".to_string(), 5)), Some(&1));
}

#[test]
fn test_max_incremental_with_pop() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 10));
    db.commit();

    let mut maxed = max(rel, |t| t.0.clone(), |t| t.1);

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
    db.pop();

    let changes3 = collect_to_map(&mut maxed);
    // Max (20) removed, old max (10) restored
    assert_eq!(changes3.get(&("a".to_string(), 20)), Some(&-1));
    assert_eq!(changes3.get(&("a".to_string(), 10)), Some(&1));
}

#[test]
fn test_count_via_sum() {
    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 1));
    handle.insert(("a".to_string(), 2));
    handle.insert(("a".to_string(), 3));
    handle.insert(("b".to_string(), 10));
    db.commit();

    // Count by mapping each tuple to 1 and summing
    let ones = map(rel, |t| (t.0.clone(), 1i64));
    let mut counted = sum(ones, |t| t.0.clone(), |t| t.1);

    let changes = collect_to_map(&mut counted);
    assert_eq!(changes.get(&("a".to_string(), 3)), Some(&1)); // 3 items with key "a"
    assert_eq!(changes.get(&("b".to_string(), 1)), Some(&1)); // 1 item with key "b"
}

#[test]
fn test_min_via_max_reverse() {
    use std::cmp::Reverse;

    let mut db = Database2::new();
    let (mut handle, rel) = db.create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 10));
    handle.insert(("a".to_string(), 20));
    handle.insert(("a".to_string(), 5));
    db.commit();

    // Min by wrapping values in Reverse and using max
    let reversed = map(rel, |t| (t.0.clone(), Reverse(t.1)));
    let mut maxed = max(reversed, |t| t.0.clone(), |t| t.1.clone());

    let changes = collect_to_map(&mut maxed);
    // Max of Reverse values is min of original values
    assert_eq!(changes.get(&("a".to_string(), Reverse(5))), Some(&1));
}

// =============================================================================
// Ported tests from database/tests.rs
// =============================================================================

mod ported_tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::super::feedback::Variable;
    use super::super::{Database2, join, map, save, union};
    use super::collect_to_map;

    #[test]
    fn test_create_and_insert() {
        let mut db = Database2::new();
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
    fn test_transitive_closure() {
        let mut db = Database2::new();
        let (mut handle, edges) = db.create_input::<(i32, i32)>();

        // Create graph: 1->2->3->4
        handle.insert((1, 2));
        handle.insert((2, 3));
        handle.insert((3, 4));
        db.commit();

        // Create the path variable
        let path_var = Rc::new(RefCell::new(Variable::<(i32, i32)>::new()));

        // path = edges ∪ (path ⋈ edges).map(|(p, e)| (p.0, e.1))
        let mut saved_edges = save(edges);
        let edges_for_union = saved_edges.get();
        let edges_for_join = saved_edges.get();

        // Create a relation that reads from the variable
        let path_rel = super::super::VariableRelation::new(path_var.clone());

        // Recursive case: extend paths by one edge
        // path(a, c) :- path(a, b), edge(b, c)
        let extended = join(path_rel, edges_for_join, |p| p.1, |e| e.0);
        let new_paths = map(extended, |((a, _), (_, c))| (a, c));

        // Combine base (edges) and recursive (new_paths)
        let all_paths = union(edges_for_union, new_paths);

        // Wire up the feedback
        db.feedback(path_var.clone(), all_paths);
        db.commit();

        // Collect results
        let result: Vec<_> = path_var.borrow().collect();

        // Should have: (1,2), (2,3), (3,4), (1,3), (2,4), (1,4)
        assert!(result.contains(&(1, 2)), "missing (1,2)");
        assert!(result.contains(&(2, 3)), "missing (2,3)");
        assert!(result.contains(&(3, 4)), "missing (3,4)");
        assert!(result.contains(&(1, 3)), "missing (1,3)");
        assert!(result.contains(&(2, 4)), "missing (2,4)");
        assert!(result.contains(&(1, 4)), "missing (1,4)");
    }

    #[test]
    fn test_push_pop_simple() {
        let mut db = Database2::new();
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
        db.pop();

        let after_pop = collect_to_map(&mut rel);
        assert_eq!(after_pop.get(&3), Some(&-1)); // deletion
    }
}
