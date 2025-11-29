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
    let (mut handle, mut rel) = create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);

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
    let (mut handle, rel) = create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);

    let mut mapped = map(rel, |x| x * 2);

    let changes = collect_to_map(&mut mapped);
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&4), Some(&1));
    assert_eq!(changes.get(&6), Some(&1));
}

#[test]
fn test_filter() {
    let (mut handle, rel) = create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);
    handle.insert(4);

    let mut filtered = filter(rel, |x| x % 2 == 0);

    let changes = collect_to_map(&mut filtered);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&4), Some(&1));
}

#[test]
fn test_flat_map() {
    let (mut handle, rel) = create_input::<i32>();

    handle.insert(1);
    handle.insert(2);

    // Each number produces itself and its double
    let mut flat_mapped = flat_map(rel, |x| vec![*x, x * 2]);

    let changes = collect_to_map(&mut flat_mapped);
    assert_eq!(changes.get(&1), Some(&1));
    assert_eq!(changes.get(&2), Some(&2)); // 2 appears twice: from 1*2 and from 2
    assert_eq!(changes.get(&4), Some(&1));
}

#[test]
fn test_union() {
    let (mut handle_a, rel_a) = create_input::<i32>();
    let (mut handle_b, rel_b) = create_input::<i32>();

    handle_a.insert(1);
    handle_a.insert(2);
    handle_b.insert(2);
    handle_b.insert(3);

    let mut unioned = union(rel_a, rel_b);

    let changes = collect_to_map(&mut unioned);
    assert_eq!(changes.get(&1), Some(&1));
    assert_eq!(changes.get(&2), Some(&2)); // 2 appears in both
    assert_eq!(changes.get(&3), Some(&1));
}

#[test]
fn test_distinct() {
    let (mut handle, rel) = create_input::<i32>();

    handle.insert(1);
    handle.insert(1);
    handle.insert(2);

    let mut distinct_rel = distinct(rel);

    let changes = collect_to_map(&mut distinct_rel);
    assert_eq!(changes.get(&1), Some(&1)); // collapsed to 1
    assert_eq!(changes.get(&2), Some(&1));
}

#[test]
fn test_distinct_incremental() {
    let (mut handle, rel) = create_input::<i32>();

    handle.insert(1);
    handle.insert(1);

    let mut distinct_rel = distinct(rel);

    // First batch: 1 appears (count goes 0 -> 2, output +1)
    let changes1 = collect_to_map(&mut distinct_rel);
    assert_eq!(changes1.get(&1), Some(&1));

    // Delete one copy of 1
    handle.delete(1);

    // Second batch: 1 still present (count goes 2 -> 1, no output change)
    let changes2 = collect_to_map(&mut distinct_rel);
    assert!(changes2.is_empty());

    // Delete the other copy
    handle.delete(1);

    // Third batch: 1 disappears (count goes 1 -> 0, output -1)
    let changes3 = collect_to_map(&mut distinct_rel);
    assert_eq!(changes3.get(&1), Some(&-1));
}

#[test]
fn test_difference() {
    let (mut handle_a, rel_a) = create_input::<i32>();
    let (mut handle_b, rel_b) = create_input::<i32>();

    handle_a.insert(1);
    handle_a.insert(2);
    handle_a.insert(3);
    handle_b.insert(2);
    handle_b.insert(4);

    let mut diff = difference(rel_a, rel_b);

    let changes = collect_to_map(&mut diff);
    assert_eq!(changes.get(&1), Some(&1));
    assert_eq!(changes.get(&3), Some(&1));
    assert_eq!(changes.get(&2), None); // 2 is in b
    assert_eq!(changes.get(&4), None); // 4 is only in b
}

#[test]
fn test_join() {
    let (mut handle_edges, rel_edges) = create_input::<(i32, i32)>();
    let (mut handle_labels, rel_labels) = create_input::<(i32, String)>();

    handle_edges.insert((1, 2));
    handle_edges.insert((2, 3));
    handle_edges.insert((3, 4));

    handle_labels.insert((1, "one".to_string()));
    handle_labels.insert((2, "two".to_string()));
    handle_labels.insert((5, "five".to_string()));

    let mut joined = join(rel_edges, rel_labels, |e| e.0, |l| l.0);

    let changes = collect_to_map(&mut joined);
    assert_eq!(changes.len(), 2);
    assert_eq!(
        changes.get(&((1, 2), (1, "one".to_string()))),
        Some(&1)
    );
    assert_eq!(
        changes.get(&((2, 3), (2, "two".to_string()))),
        Some(&1)
    );
}

#[test]
fn test_join_incremental() {
    let (mut handle_a, rel_a) = create_input::<(i32, i32)>();
    let (mut handle_b, rel_b) = create_input::<(i32, i32)>();

    handle_a.insert((1, 10));

    let mut joined = join(rel_a, rel_b, |a| a.0, |b| b.0);

    // First batch: no matches yet
    let changes1 = collect_to_map(&mut joined);
    assert!(changes1.is_empty());

    // Add matching tuple to b
    handle_b.insert((1, 20));

    // Second batch: now we have a match
    let changes2 = collect_to_map(&mut joined);
    assert_eq!(changes2.get(&((1, 10), (1, 20))), Some(&1));
}

#[test]
fn test_chained_operators() {
    let (mut handle, rel) = create_input::<i32>();

    handle.insert(1);
    handle.insert(2);
    handle.insert(3);
    handle.insert(4);

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
    let (mut handle, rel) = create_input::<i32>();

    handle.insert(1);
    handle.insert(2);

    // Box to break type chain
    let boxed = rel.boxed();
    let mut mapped = map(boxed, |x| x * 2);

    let changes = collect_to_map(&mut mapped);
    assert_eq!(changes.get(&2), Some(&1));
    assert_eq!(changes.get(&4), Some(&1));
}

#[test]
fn test_saved_relation() {
    let (mut handle, rel) = create_input::<i32>();

    handle.insert(1);
    handle.insert(2);

    let mut saved = save(rel);

    // Get two consumers
    let mut getter1 = saved.get();
    let mut getter2 = saved.get();

    // Pull from upstream
    saved.update();

    // Both should see the same changes
    let changes1 = collect_to_map(&mut getter1);
    let changes2 = collect_to_map(&mut getter2);

    assert_eq!(changes1, changes2);
    assert_eq!(changes1.get(&1), Some(&1));
    assert_eq!(changes1.get(&2), Some(&1));

    // After both consumed, changes should be cleared
    // Add more data
    handle.insert(3);
    saved.update();

    let changes3 = collect_to_map(&mut getter1);
    let changes4 = collect_to_map(&mut getter2);

    assert_eq!(changes3, changes4);
    assert_eq!(changes3.get(&3), Some(&1));
    assert_eq!(changes3.get(&1), None); // old changes gone
}

#[test]
fn test_self_join_with_saved() {
    let (mut handle, rel) = create_input::<(i32, i32)>();

    // Create a simple path: 1 -> 2 -> 3
    handle.insert((1, 2));
    handle.insert((2, 3));

    let mut saved = save(rel);
    let left = saved.get();
    let right = saved.get();
    saved.update();

    // Self-join: find paths of length 2
    let mut joined = join(left, right, |e| e.1, |e| e.0);

    let changes = collect_to_map(&mut joined);
    // (1,2) joins with (2,3) giving us path 1 -> 2 -> 3
    assert_eq!(changes.get(&((1, 2), (2, 3))), Some(&1));
}

#[test]
fn test_sum() {
    let (mut handle, rel) = create_input::<(String, i64)>();

    handle.insert(("a".to_string(), 10));
    handle.insert(("a".to_string(), 20));
    handle.insert(("b".to_string(), 5));

    let mut summed = sum(rel, |t| t.0.clone(), |t| t.1);

    let changes = collect_to_map(&mut summed);
    // Net result: ("a", 30) and ("b", 5)
    assert_eq!(changes.get(&("a".to_string(), 30)), Some(&1));
    assert_eq!(changes.get(&("b".to_string(), 5)), Some(&1));
}

#[test]
fn test_sum_incremental() {
    let (mut handle, rel) = create_input::<(String, i64)>();

    handle.insert(("a".to_string(), 10));

    let mut summed = sum(rel, |t| t.0.clone(), |t| t.1);

    let changes1 = collect_to_map(&mut summed);
    assert_eq!(changes1.get(&("a".to_string(), 10)), Some(&1));

    // Add more to "a"
    handle.insert(("a".to_string(), 5));

    let changes2 = collect_to_map(&mut summed);
    // Old sum deleted, new sum inserted
    assert_eq!(changes2.get(&("a".to_string(), 10)), Some(&-1));
    assert_eq!(changes2.get(&("a".to_string(), 15)), Some(&1));
}

#[test]
fn test_max() {
    let (mut handle, rel) = create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 10));
    handle.insert(("a".to_string(), 20));
    handle.insert(("a".to_string(), 15));
    handle.insert(("b".to_string(), 5));

    let mut maxed = max(rel, |t| t.0.clone(), |t| t.1);

    let changes = collect_to_map(&mut maxed);
    // Max of "a" is 20, max of "b" is 5
    assert_eq!(changes.get(&("a".to_string(), 20)), Some(&1));
    assert_eq!(changes.get(&("b".to_string(), 5)), Some(&1));
}

#[test]
fn test_max_incremental() {
    let (mut handle, rel) = create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 10));
    handle.insert(("a".to_string(), 20));

    let mut maxed = max(rel, |t| t.0.clone(), |t| t.1);

    let changes1 = collect_to_map(&mut maxed);
    assert_eq!(changes1.get(&("a".to_string(), 20)), Some(&1));

    // Delete the max value
    handle.delete(("a".to_string(), 20));

    let changes2 = collect_to_map(&mut maxed);
    // Old max deleted, new max (10) inserted
    assert_eq!(changes2.get(&("a".to_string(), 20)), Some(&-1));
    assert_eq!(changes2.get(&("a".to_string(), 10)), Some(&1));
}

#[test]
fn test_count_via_sum() {
    let (mut handle, rel) = create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 1));
    handle.insert(("a".to_string(), 2));
    handle.insert(("a".to_string(), 3));
    handle.insert(("b".to_string(), 10));

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

    let (mut handle, rel) = create_input::<(String, i32)>();

    handle.insert(("a".to_string(), 10));
    handle.insert(("a".to_string(), 20));
    handle.insert(("a".to_string(), 5));

    // Min by wrapping values in Reverse and using max
    let reversed = map(rel, |t| (t.0.clone(), Reverse(t.1)));
    let mut maxed = max(reversed, |t| t.0.clone(), |t| t.1.clone());

    let changes = collect_to_map(&mut maxed);
    // Max of Reverse values is min of original values
    assert_eq!(changes.get(&("a".to_string(), Reverse(5))), Some(&1));
}

// =============================================================================
// Feedback / Fixpoint tests
// =============================================================================

mod feedback_tests {
    use super::super::feedback::{fixpoint, Iteration, Variable};
    use crate::change::Diff;

    #[test]
    fn test_variable_basic() {
        let mut var = Variable::new();

        // Insert some values
        var.insert(1);
        var.insert(2);
        var.insert(1); // duplicate

        // Collect all positive tuples
        let result: Vec<_> = var.collect();
        assert!(result.contains(&1));
        assert!(result.contains(&2));
    }

    #[test]
    fn test_variable_emits_once() {
        let mut var = Variable::<i32>::new();

        // First insert triggers emission
        var.insert(1);
        let changes1 = var.take_changes();
        assert_eq!(changes1.len(), 1);
        assert_eq!(changes1[0], (1, Diff(1)));

        // Second insert of same value does NOT emit (already positive)
        var.insert(1);
        let changes2 = var.take_changes();
        assert!(changes2.is_empty());
    }

    #[test]
    fn test_simple_fixpoint() {
        // Test fixpoint with simple doubling until we reach 8
        let mut var = Variable::new();
        var.insert(1);

        let (result, iterations) = fixpoint(
            var,
            |changes| {
                let mut new_changes = Vec::new();
                for (val, diff) in changes {
                    if diff.0 > 0 && *val < 8 {
                        new_changes.push((val * 2, Diff(1)));
                    }
                }
                new_changes
            },
            100,
        );

        // Should have 1, 2, 4, 8
        let values: Vec<_> = result.collect();
        assert!(values.contains(&1));
        assert!(values.contains(&2));
        assert!(values.contains(&4));
        assert!(values.contains(&8));
        assert!(!values.contains(&16)); // stopped at 8

        // Iterations:
        // 1: take initial (1), produce (2)
        // 2: take (2), produce (4)
        // 3: take (4), produce (8)
        // 4: take (8), produce nothing (8 < 8 is false)
        assert_eq!(iterations, 4);
    }

    #[test]
    fn test_transitive_closure_manual() {
        // Manual transitive closure using Iteration
        // Graph: 1->2->3->4
        let mut iter = Iteration::new();

        // Add initial edges as paths
        iter.insert((1, 2));
        iter.insert((2, 3));
        iter.insert((3, 4));

        // Edges for joining (static)
        let edges: Vec<(i32, i32)> = vec![(1, 2), (2, 3), (3, 4)];

        // Run fixpoint: for each path (a, b) and edge (b, c), add path (a, c)
        let iterations = iter.run(
            |changes, _totals| {
                let mut new_paths = Vec::new();
                for ((a, b), diff) in changes {
                    if diff.0 > 0 {
                        // Find edges starting from b
                        for &(eb, ec) in &edges {
                            if eb == *b {
                                new_paths.push(((*a, ec), Diff(1)));
                            }
                        }
                    }
                }
                new_paths
            },
            100,
        );

        let paths: Vec<_> = iter.result().collect();

        // Direct edges
        assert!(paths.contains(&(1, 2)));
        assert!(paths.contains(&(2, 3)));
        assert!(paths.contains(&(3, 4)));

        // 2-hop paths
        assert!(paths.contains(&(1, 3)));
        assert!(paths.contains(&(2, 4)));

        // 3-hop path
        assert!(paths.contains(&(1, 4)));

        // Should be 3 iterations: initial, +2-hops, +3-hop
        assert_eq!(iterations, 3);
    }

    #[test]
    fn test_fixpoint_with_relational_ops() {
        // Use the relational operators inside fixpoint
        use super::super::{create_input, save, Relation};

        // Build transitive closure using relational operators

        // Edges input
        let (mut edges_handle, edges_rel) = create_input::<(i32, i32)>();

        // Graph: 1->2->3
        edges_handle.insert((1, 2));
        edges_handle.insert((2, 3));

        // For fixpoint, we need to manually iterate
        // First, collect edges into our Variable
        let mut path_var = Variable::<(i32, i32)>::new();

        // Initial: paths = edges
        let mut edges_saved = save(edges_rel);
        let mut edges_getter = edges_saved.get();
        edges_saved.update();

        // Drain edges into path_var
        edges_getter.foreach(&mut |t, diff| {
            if diff.0 > 0 {
                path_var.insert(t.clone());
            }
        });

        // Now we need to run fixpoint
        // For each iteration, we join current paths with edges
        let edges_for_join: Vec<_> = vec![(1, 2), (2, 3)];

        let (result, _iterations) = fixpoint(
            path_var,
            |changes| {
                // Join path changes with edges: (a, b) ⋈ (b, c) -> (a, c)
                let mut new_paths = Vec::new();
                for ((a, b), diff) in changes {
                    if diff.0 > 0 {
                        for &(eb, ec) in &edges_for_join {
                            if *b == eb {
                                new_paths.push(((*a, ec), Diff(1)));
                            }
                        }
                    }
                }
                new_paths
            },
            100,
        );

        let paths: Vec<_> = result.collect();
        assert!(paths.contains(&(1, 2)));
        assert!(paths.contains(&(2, 3)));
        assert!(paths.contains(&(1, 3))); // transitive path!
    }

    #[test]
    fn test_stratified_multi_feedback() {
        // Demonstrate the stratified fixpoint algorithm:
        // 'outer: loop {
        //   for feedback in feedbacks {
        //     feedback.apply()
        //     if changed { continue 'outer }
        //   }
        //   break
        // }

        // We'll have two independent transitive closures
        let mut var1 = Variable::<(i32, i32)>::new();
        let mut var2 = Variable::<(char, char)>::new();

        // Graph 1: 1->2->3
        var1.insert((1, 2));
        var1.insert((2, 3));

        // Graph 2: a->b->c
        var2.insert(('a', 'b'));
        var2.insert(('b', 'c'));

        let edges1: Vec<(i32, i32)> = vec![(1, 2), (2, 3)];
        let edges2: Vec<(char, char)> = vec![('a', 'b'), ('b', 'c')];

        let max_iterations = 100;
        let mut total_iterations = 0;

        // Stratified fixpoint loop
        'outer: loop {
            if total_iterations >= max_iterations {
                panic!("exceeded max iterations");
            }

            // Feedback 1: paths in graph 1
            let changes1 = var1.take_changes();
            if !changes1.is_empty() {
                total_iterations += 1;
                for ((a, b), diff) in &changes1 {
                    if diff.0 > 0 {
                        for &(eb, ec) in &edges1 {
                            if *b == eb {
                                var1.insert((*a, ec));
                            }
                        }
                    }
                }
                continue 'outer;
            }

            // Feedback 2: paths in graph 2
            let changes2 = var2.take_changes();
            if !changes2.is_empty() {
                total_iterations += 1;
                for ((a, b), diff) in &changes2 {
                    if diff.0 > 0 {
                        for &(eb, ec) in &edges2 {
                            if *b == eb {
                                var2.insert((*a, ec));
                            }
                        }
                    }
                }
                continue 'outer;
            }

            // No changes in any feedback, we're done
            break;
        }

        // Verify both graphs reached transitive closure
        let paths1: Vec<_> = var1.collect();
        assert!(paths1.contains(&(1, 2)));
        assert!(paths1.contains(&(2, 3)));
        assert!(paths1.contains(&(1, 3)));

        let paths2: Vec<_> = var2.collect();
        assert!(paths2.contains(&('a', 'b')));
        assert!(paths2.contains(&('b', 'c')));
        assert!(paths2.contains(&('a', 'c')));
    }

    #[test]
    fn test_variable_deletion() {
        // Test that deletions work correctly
        let mut var = Variable::<i32>::new();

        // Add two copies of 1
        var.insert(1);
        var.insert(1);

        // Take changes (should emit +1 once)
        let changes = var.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0], (1, Diff(1)));

        // Delete one copy - still positive, no output
        var.add_change(1, Diff(-1));
        let changes = var.take_changes();
        assert!(changes.is_empty());

        // Delete the other copy - now 0, emit -1
        var.add_change(1, Diff(-1));
        let changes = var.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0], (1, Diff(-1)));

        // Verify the value is gone
        let values: Vec<_> = var.collect();
        assert!(values.is_empty());
    }
}
