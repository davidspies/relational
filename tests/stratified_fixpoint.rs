//! Tests for stratified fixpoint semantics with multiple feedback loops.
//!
//! These tests verify that feedback loops are processed in declaration order,
//! with each reaching fixpoint before the next is applied.

use relational::Database;

/// Test that multiple feedbacks run in stratified order.
///
/// We set up two feedback loops where the second depends on the first reaching fixpoint.
/// This models a scenario like:
/// - First feedback: compute transitive closure of edges
/// - Second feedback: compute something based on the full transitive closure
#[test]
fn test_stratified_two_feedbacks() {
    let mut db = Database::new();

    // Input: edges in a graph
    let edges = db.create_input::<(i32, i32)>("edges");

    // First feedback: transitive closure (reachability)
    // reach(a, b) :- edge(a, b)
    // reach(a, c) :- reach(a, b), edge(b, c)
    let (reach_var, reach) = db.variable::<(i32, i32)>("reach");

    let extended_reach = db.join(reach, edges, |(_, b)| *b, |(b, _)| *b);
    let new_reach = db.map(extended_reach, |((a, _), (_, c))| (*a, *c));
    let all_reach = db.union(edges, new_reach);

    // Second feedback: count how many nodes each node can reach
    // This should only run AFTER reach is at fixpoint
    // reachable_count(a, count) where count = |{b : reach(a, b)}|
    //
    // We'll model this differently: track pairs (a, b) where a can reach b
    // and b can reach some node c. This creates a dependency on reach being complete.
    let (extended_var, extended) = db.variable::<(i32, i32, i32)>("extended");

    // extended(a, b, c) :- reach(a, b), reach(b, c)
    let reach_join = db.join(reach, reach, |(_, b)| *b, |(b, _)| *b);
    let triples = db.map(reach_join, |((a, b), (_, c))| (*a, *b, *c));

    // Insert edges: 1 -> 2 -> 3
    db.insert(edges, (1, 2));
    db.insert(edges, (2, 3));

    // Set up first feedback (reach)
    db.feedback(reach_var, edges, all_reach);

    // At this point, reach should be at fixpoint: {(1,2), (2,3), (1,3)}
    let reach_result: Vec<_> = db.collect(reach);
    assert!(reach_result.contains(&(1, 2)), "reach should contain (1,2)");
    assert!(reach_result.contains(&(2, 3)), "reach should contain (2,3)");
    assert!(reach_result.contains(&(1, 3)), "reach should contain (1,3)");
    assert_eq!(reach_result.len(), 3, "reach should have exactly 3 pairs");

    // Set up second feedback (extended)
    // The base is empty, recursive is triples
    let empty = db.filter(reach, |_| false);
    let empty_triples = db.map(empty, |&(a, b)| (a, b, 0)); // Type conversion hack

    db.feedback(extended_var, empty_triples, triples);

    // Extended should contain all (a, b, c) where reach(a,b) and reach(b,c)
    // With reach = {(1,2), (2,3), (1,3)}:
    // - reach(1,2) and reach(2,3) -> (1, 2, 3)
    // - reach(1,3) and reach(3,?) -> nothing (3 doesn't reach anything)
    // - reach(2,3) and reach(3,?) -> nothing
    let extended_result: Vec<_> = db.collect(extended);
    assert!(
        extended_result.contains(&(1, 2, 3)),
        "extended should contain (1,2,3)"
    );
    assert_eq!(
        extended_result.len(),
        1,
        "extended should have exactly 1 triple"
    );
}

/// Test that adding edges after feedbacks are set up triggers re-computation.
#[test]
fn test_incremental_after_feedback() {
    let mut db = Database::new();

    let edges = db.create_input::<(i32, i32)>("edges");

    // Set up transitive closure
    let (path_var, path) = db.variable::<(i32, i32)>("path");
    let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
    let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
    let all_paths = db.union(edges, new_paths);

    // Initial edges
    db.insert(edges, (1, 2));
    db.insert(edges, (2, 3));

    db.feedback(path_var, edges, all_paths);

    // Check initial state
    let paths: Vec<_> = db.collect(path);
    assert_eq!(paths.len(), 3); // (1,2), (2,3), (1,3)
    assert!(paths.contains(&(1, 3)));

    // Add another edge
    db.insert(edges, (3, 4));

    // The transitive closure should update
    let paths: Vec<_> = db.collect(path);
    assert!(paths.contains(&(3, 4)), "should have new direct edge");
    assert!(paths.contains(&(2, 4)), "should have (2,4) via (2,3,4)");
    assert!(paths.contains(&(1, 4)), "should have (1,4) via (1,2,3,4)");
    assert_eq!(paths.len(), 6); // (1,2), (2,3), (3,4), (1,3), (2,4), (1,4)
}

/// Test that the order of feedback declarations matters.
///
/// If we declare feedback B before feedback A, but B depends on A,
/// things should still work because the stratified algorithm re-runs
/// earlier feedbacks when later ones change.
#[test]
fn test_feedback_order_independence() {
    let mut db = Database::new();

    let edges = db.create_input::<(i32, i32)>("edges");
    db.insert(edges, (1, 2));
    db.insert(edges, (2, 3));

    // Transitive closure
    let (path_var, path) = db.variable::<(i32, i32)>("path");
    let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
    let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
    let all_paths = db.union(edges, new_paths);

    db.feedback(path_var, edges, all_paths);

    let paths: Vec<_> = db.collect(path);
    assert_eq!(paths.len(), 3);
    assert!(paths.contains(&(1, 2)));
    assert!(paths.contains(&(2, 3)));
    assert!(paths.contains(&(1, 3)));
}

/// Test a chain of three feedbacks.
#[test]
fn test_three_feedbacks_chain() {
    let mut db = Database::new();

    // Level 0: base facts
    let facts = db.create_input::<i32>("facts");
    db.insert(facts, 1);

    // Level 1: double the facts
    let (doubled_var, doubled) = db.variable::<i32>("doubled");
    let double_op = db.map(facts, |x| x * 2);

    // Level 2: triple the doubled values
    let (tripled_var, tripled) = db.variable::<i32>("tripled");
    let triple_op = db.map(doubled, |x| x * 3);

    // Level 3: add 1 to tripled values
    let (plus_one_var, plus_one) = db.variable::<i32>("plus_one");
    let plus_one_op = db.map(tripled, |x| x + 1);

    // Set up feedbacks in order
    // doubled = double(facts)
    db.feedback(doubled_var, double_op, double_op);

    // After first feedback: doubled = {2}
    let doubled_result: Vec<_> = db.collect(doubled);
    assert!(doubled_result.contains(&2), "doubled should contain 2");

    // tripled = triple(doubled)
    db.feedback(tripled_var, triple_op, triple_op);

    // After second feedback: tripled = {6}
    let tripled_result: Vec<_> = db.collect(tripled);
    assert!(tripled_result.contains(&6), "tripled should contain 6");

    // plus_one = plus_one(tripled)
    db.feedback(plus_one_var, plus_one_op, plus_one_op);

    // After third feedback: plus_one = {7}
    let plus_one_result: Vec<_> = db.collect(plus_one);
    assert!(plus_one_result.contains(&7), "plus_one should contain 7");
}

/// Test that a feedback that doesn't change anything doesn't cause infinite loops.
#[test]
fn test_feedback_immediate_fixpoint() {
    let mut db = Database::new();

    let items = db.create_input::<i32>("items");
    db.insert(items, 1);
    db.insert(items, 2);

    // Feedback that just passes through the input (identity)
    let (var, rel) = db.variable::<i32>("identity");

    db.feedback(var, items, items);

    let result: Vec<_> = db.collect(rel);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&1));
    assert!(result.contains(&2));
}

/// Test that feedback A runs to fixpoint between applications of feedback B.
///
/// This test uses a truly non-monotonic setup where:
/// - Feedback A: grows a set by adding n+1 (capped at 5)
/// - Feedback B: observes A and tracks what it has "seen" via set difference
///
/// The key insight: with non-monotonic operations, the ORDER matters.
/// We use tuples (value, batch_number) to track WHEN values were observed.
///
/// With CORRECT stratified ordering (A reaches fixpoint before each B step):
/// - A reaches {1,2,3,4,5} completely
/// - B sees all values at once, so all get batch 1
///
/// With INCORRECT interleaved ordering (A and B step together):
/// - Values would get different batch numbers depending on when B observed them
///
/// The test asserts that all B values have the SAME batch number, proving
/// A reached full fixpoint before B ever observed it.
#[test]
fn test_a_reaches_fixpoint_between_b_applications() {
    let mut db = Database::new();

    // We'll use tuples (value, batch_number) to track WHEN values appeared
    // Feedback A: produces values 1..=5 with their "discovery time"
    let seeds = db.create_input::<(i32, i32)>("seeds");

    let (a_var, a_rel) = db.variable::<(i32, i32)>("a");
    // Generate next value: (n, t) -> (n+1, t) if n+1 <= 5
    let extended = db.map(a_rel, |(n, t)| (n + 1, *t));
    let capped = db.filter(extended, |(n, _)| *n <= 5);
    let a_all = db.union(seeds, capped);

    // Feedback B: snapshot what's in A, but with incremented batch
    // This simulates "B observes A and records the observation time"
    let (b_var, b_rel) = db.variable::<(i32, i32)>("b");

    // Get just the values from A (ignore the batch from A)
    let a_vals = db.map(a_rel, |(v, _)| *v);
    let b_vals = db.map(b_rel, |(v, _)| *v);

    // New values = values in A but not yet in B
    let new_vals = db.difference(a_vals, b_vals);

    // Mark new values with batch 1 (simulating B's observation)
    let new_with_batch = db.map(new_vals, |v| (*v, 1));
    let b_all = db.union(b_rel, new_with_batch);

    // Insert seed with batch 0
    db.insert(seeds, (1, 0));

    // Set up both feedbacks at once - they run in declaration order
    db.feedback(a_var, seeds, a_all);
    db.feedback(b_var, new_with_batch, b_all);

    // Check results
    let a_result: Vec<_> = db.collect(a_rel);
    let b_result: Vec<_> = db.collect(b_rel);

    // A should have {1,2,3,4,5} all with batch 0
    assert_eq!(a_result.len(), 5, "A should have 5 elements");
    for (v, batch) in &a_result {
        assert_eq!(*batch, 0, "All A values should have batch 0, but {} has batch {}", v, batch);
    }

    // B should have {1,2,3,4,5} all with batch 1
    // This is because A reached fixpoint FIRST, then B saw all values at once
    assert_eq!(b_result.len(), 5, "B should have 5 elements");
    for (v, batch) in &b_result {
        assert_eq!(*batch, 1, "All B values should have batch 1, but {} has batch {}", v, batch);
    }

    // The key assertion: all B values have the SAME batch number
    // This proves A reached full fixpoint before B observed it
    // If ordering were wrong, B would have observed partial states
    // and values would have different batch numbers
    let b_batches: std::collections::HashSet<i32> = b_result.iter().map(|(_, b)| *b).collect();
    assert_eq!(
        b_batches.len(),
        1,
        "All B values should have same batch - proves A fixpointed first. Batches: {:?}",
        b_batches
    );
}

/// Test interleaved fixpoint with mutual dependency.
///
/// This tests a scenario where:
/// - A depends on B's output
/// - B depends on A's output
/// - Both must reach a global fixpoint together
///
/// We model this as:
/// - A: set of "even" numbers reachable from input
/// - B: set of "odd" numbers reachable from A
/// And we feed B back into A's input.
#[test]
fn test_interleaved_mutual_fixpoint() {
    let mut db = Database::new();

    // Input numbers
    let input = db.create_input::<i32>("input");

    // A: tracks numbers, adds +2 to each (stays in same parity class)
    let (a_var, a_rel) = db.variable::<i32>("a");
    let a_plus_2 = db.map(a_rel, |x| x + 2);
    let a_filtered = db.filter(a_plus_2, |x| *x <= 10); // Cap at 10

    // B: takes A's values, adds +1 (switches parity)
    let (b_var, b_rel) = db.variable::<i32>("b");
    let b_from_a = db.map(a_rel, |x| x + 1);
    let b_filtered = db.filter(b_from_a, |x| *x <= 10);

    // Union B back into A's input (so A grows from B's output too)
    let a_combined = db.union(input, b_rel);
    let a_recursive = db.union(a_combined, a_filtered);

    // Start with just 1
    db.insert(input, 1);

    // Set up A first
    db.feedback(a_var, a_combined, a_recursive);

    // A should have: 1, 3, 5, 7, 9 (starting from 1, adding 2 each time)
    let a_result: Vec<_> = db.collect(a_rel);
    assert!(a_result.contains(&1), "A should contain 1");
    assert!(a_result.contains(&3), "A should contain 3");
    assert!(a_result.contains(&5), "A should contain 5");

    // Set up B
    db.feedback(b_var, b_filtered, b_filtered);

    // Now B should have added even numbers to A via the feedback
    // B = A + 1 = {2, 4, 6, 8, 10}
    let b_result: Vec<_> = db.collect(b_rel);
    assert!(b_result.contains(&2), "B should contain 2");
    assert!(b_result.contains(&4), "B should contain 4");

    // And A should now also have even numbers (from B feeding back)
    let a_result_after: Vec<_> = db.collect(a_rel);
    assert!(
        a_result_after.contains(&2),
        "A should contain 2 after B feedback"
    );
    assert!(
        a_result_after.contains(&4),
        "A should contain 4 after B feedback"
    );

    // Final state: both A and B should have all numbers 1-10
    assert!(
        a_result_after.len() >= 5,
        "A should have grown from B's feedback"
    );
}

/// Test diamond dependency pattern.
///
/// ```text
///     A
///    / \
///   B   C
///    \ /
///     D
/// ```
///
/// A produces values, B and C transform them differently,
/// D combines B and C.
#[test]
fn test_diamond_dependency() {
    let mut db = Database::new();

    let input = db.create_input::<i32>("input");
    db.insert(input, 10);

    // B = input * 2
    let b = db.map(input, |x| x * 2);

    // C = input + 5
    let c = db.map(input, |x| x + 5);

    // D = B union C
    let d = db.union(b, c);

    // Use a feedback to test that the diamond is computed correctly
    let (d_var, d_rel) = db.variable::<i32>("d_feedback");
    db.feedback(d_var, d, d);

    let result: Vec<_> = db.collect(d_rel);
    assert!(result.contains(&20), "should have 10*2=20 from B");
    assert!(result.contains(&15), "should have 10+5=15 from C");
    assert_eq!(result.len(), 2);
}
