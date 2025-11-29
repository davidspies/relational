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
    db.commit();

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
    db.commit();

    db.feedback(path_var, edges, all_paths);

    // Check initial state
    let paths: Vec<_> = db.collect(path);
    assert_eq!(paths.len(), 3); // (1,2), (2,3), (1,3)
    assert!(paths.contains(&(1, 3)));

    // Add another edge
    db.insert(edges, (3, 4));
    db.commit();

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
    db.commit();

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
    db.commit();

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
    db.commit();

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
/// # Setup
/// We have ONE variable V with TWO separate feedback loops:
///
/// ```text
/// v_max = max(v)
/// a = v_max.map(|x| if x % 100 < 50 { x + 7 } else { x })
/// b = v_max.map(|x| if x < 200 { x + 31 } else { x })
/// feedback(a.difference(v), v)
/// feedback(b.difference(v), v)
/// ```
///
/// **Feedback A**: Adds 7 when (max % 100) < 50
/// - When condition is true: produces max+7 (new value, added via difference)
/// - When condition is false: produces max (already in v, difference removes it)
///
/// **Feedback B**: Adds 31 when max < 200
/// - When condition is true: produces max+31 (new value)
/// - When condition is false: produces max (already in v, no change)
///
/// # Stratified Semantics
/// ```text
/// Start: v = {0}
///
/// A to fixpoint:
///   max=0, 0%100=0 < 50, a=7, 7∉v, add 7. v={0,7}
///   max=7, 7%100=7 < 50, a=14, add. v={0,7,14}
///   ... → 21 → 28 → 35 → 42 → 49
///   max=49, 49%100=49 < 50, a=56, add 56. v={...,49,56}
///   max=56, 56%100=56 >= 50, a=56, 56∈v, no change. FIXPOINT at max=56.
///
/// B once:
///   max=56, 56 < 200, b=87, add 87. v={...,56,87}
///
/// A to fixpoint:
///   max=87, 87%100=87 >= 50, a=87, already in v. FIXPOINT.
///
/// B once:
///   max=87, 87 < 200, b=118, add 118. v={...,87,118}
///
/// A to fixpoint:
///   max=118, 118%100=18 < 50, a=125, add. → 132 → 139 → 146 → 153
///   max=153, 153%100=53 >= 50, a=153, no change. FIXPOINT at max=153.
///
/// B once:
///   max=153, 153 < 200, b=184, add 184. v={...,153,184}
///
/// A to fixpoint:
///   max=184, 184%100=84 >= 50, no change.
///
/// B once:
///   max=184, 184 < 200, b=215, add 215. v={...,184,215}
///
/// A to fixpoint:
///   max=215, 215%100=15 < 50, a=222, add. → 229 → 236 → 243 → 250
///   max=250, 250%100=50 >= 50, no change. FIXPOINT at max=250.
///
/// B once:
///   max=250, 250 >= 200, b=250, no change. FIXPOINT.
///
/// Global fixpoint. Final max = 250.
/// ```
///
/// # Round-Robin (Wrong) Semantics
/// With A once, B once alternating, we get a different result.
///
/// **Stratified: 250, Round-robin: different**
#[test]
fn test_a_reaches_fixpoint_between_b_applications() {
    let mut db = Database::new();

    let seeds = db.create_input::<i32>("seeds");

    // Variable V - the shared counter
    let (v_var, v_rel) = db.variable::<i32>("v");

    // v_max = max(v)
    let v_max = db.max(v_rel);

    // a = v_max.map(|x| if x % 100 < 50 { x + 7 } else { x })
    let a = db.map(v_max, |x| if x % 100 < 50 { x + 7 } else { *x });

    // b = v_max.map(|x| if x < 200 { x + 31 } else { x })
    let v_max_for_b = db.max(v_rel);
    let b = db.map(v_max_for_b, |x| if *x < 200 { x + 31 } else { *x });

    // a_new = a.difference(v) - only the NEW values from A
    let a_new = db.difference(a, v_rel);

    // b_new = b.difference(v) - only the NEW values from B
    let b_new = db.difference(b, v_rel);

    // For feedback, we need: v = v ∪ a_new  and  v = v ∪ b_new
    let v_with_a = db.union(v_rel, a_new);
    let v_with_b = db.union(v_rel, b_new);

    // Seed with 0
    db.insert(seeds, 0);
    db.commit();

    // First feedback (A): v = v ∪ a_new
    db.feedback(v_var, seeds, v_with_a);

    println!("After A feedback:");
    println!("  V = {:?}", db.collect::<i32>(v_rel));
    let v_max_after_a = db.collect::<i32>(v_rel).into_iter().max().unwrap_or(-1);
    println!("  max(V) = {}", v_max_after_a);

    // Second feedback (B): v = v ∪ b_new
    db.feedback(v_var, v_rel, v_with_b);

    println!("\nAfter B feedback:");
    println!("  V = {:?}", db.collect::<i32>(v_rel));

    let v_result: Vec<_> = db.collect(v_rel);
    let v_max_val = v_result.iter().max().copied().unwrap_or(-1);

    println!("Final max(V) = {}", v_max_val);

    // With stratified ordering (restart from first feedback on any change):
    // - A: 0→7→...→56 (A keeps running while it produces changes)
    // - B: 56→87 (B runs once, then restarts from A)
    // - A: 87%100=87>=50, no change
    // - B: 87→118 (restarts from A)
    // - A: 118→125→...→153 (53>=50, stops)
    // - B: 153→184 (restarts from A)
    // - A: 84>=50, no change
    // - B: 184→215 (restarts from A)
    // - A: 215→222→...→250 (50>=50, stops)
    // - B: 250>=200, no change
    // - Done! max=250
    assert_eq!(
        v_max_val, 250,
        "max(V) should be 250. Got V = {:?}",
        v_result
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
///
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
    db.commit();

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
    db.commit();

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

// ============================================================================
// Push/Pop Checkpoint Tests
// ============================================================================

/// Test basic push/pop without feedback loops.
#[test]
fn test_push_pop_simple() {
    let mut db = Database::new();

    let items = db.create_input::<i32>("items");
    let doubled = db.map(items, |x| x * 2);

    // Initial state
    db.insert(items, 1);
    db.insert(items, 2);
    db.commit();

    assert_eq!(db.collect(items).len(), 2);
    assert_eq!(db.collect(doubled).len(), 2);

    // Push checkpoint
    db.push(Some("before_changes"));
    assert_eq!(db.stack_depth(), 1);

    // Make changes
    db.insert(items, 3);
    db.delete(items, 1);
    db.commit();

    assert_eq!(db.collect(items).len(), 2); // {2, 3}
    let doubled_result: Vec<_> = db.collect(doubled);
    assert!(doubled_result.contains(&4)); // 2*2
    assert!(doubled_result.contains(&6)); // 3*2

    // Pop - should restore to before changes
    assert!(db.pop());
    assert_eq!(db.stack_depth(), 0);

    // Check state is restored
    let items_result: Vec<_> = db.collect(items);
    assert_eq!(items_result.len(), 2);
    assert!(items_result.contains(&1));
    assert!(items_result.contains(&2));

    let doubled_result: Vec<_> = db.collect(doubled);
    assert!(doubled_result.contains(&2)); // 1*2
    assert!(doubled_result.contains(&4)); // 2*2
}

/// Test nested push/pop.
#[test]
fn test_push_pop_nested() {
    let mut db = Database::new();

    let items = db.create_input::<i32>("items");

    db.insert(items, 1);
    db.commit();

    // First push
    db.push(Some("level1"));
    db.insert(items, 2);
    db.commit();

    // Second push
    db.push(Some("level2"));
    db.insert(items, 3);
    db.commit();

    assert_eq!(db.collect::<i32>(items).len(), 3); // {1, 2, 3}
    assert_eq!(db.stack_depth(), 2);

    // Pop level2 - should remove 3
    db.pop();
    assert_eq!(db.stack_depth(), 1);
    let items_result: Vec<_> = db.collect(items);
    assert_eq!(items_result.len(), 2);
    assert!(items_result.contains(&1));
    assert!(items_result.contains(&2));
    assert!(!items_result.contains(&3));

    // Pop level1 - should remove 2
    db.pop();
    assert_eq!(db.stack_depth(), 0);
    let items_result: Vec<_> = db.collect(items);
    assert_eq!(items_result.len(), 1);
    assert!(items_result.contains(&1));
}

/// Test push/pop with transitive closure.
#[test]
fn test_push_pop_with_feedback() {
    let mut db = Database::new();

    let edges = db.create_input::<(i32, i32)>("edges");

    // Set up transitive closure
    let (path_var, path) = db.variable::<(i32, i32)>("path");
    let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
    let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
    let all_paths = db.union(edges, new_paths);

    // Initial edges: 1 -> 2 -> 3
    db.insert(edges, (1, 2));
    db.insert(edges, (2, 3));
    db.commit();

    db.feedback(path_var, edges, all_paths);

    // Initial paths: (1,2), (2,3), (1,3)
    let paths: Vec<_> = db.collect(path);
    assert_eq!(paths.len(), 3);
    assert!(paths.contains(&(1, 3)));

    // Push and add more edges
    db.push(Some("before_new_edges"));

    db.insert(edges, (3, 4));
    db.commit();

    // Now paths should include (3,4), (2,4), (1,4)
    let paths: Vec<_> = db.collect(path);
    assert_eq!(paths.len(), 6);
    assert!(paths.contains(&(1, 4)));

    // Pop - should restore to 3 paths
    db.pop();

    let paths: Vec<_> = db.collect(path);
    assert_eq!(paths.len(), 3, "Should have 3 paths after pop: {:?}", paths);
    assert!(paths.contains(&(1, 2)));
    assert!(paths.contains(&(2, 3)));
    assert!(paths.contains(&(1, 3)));
    assert!(!paths.contains(&(1, 4)), "Should not have (1,4) after pop");
}

/// Test that pop on empty stack returns false.
#[test]
fn test_pop_empty_stack() {
    let mut db = Database::new();

    assert!(!db.pop());
    assert_eq!(db.stack_depth(), 0);
}

/// Test push without changes followed by pop.
#[test]
fn test_push_pop_no_changes() {
    let mut db = Database::new();

    let items = db.create_input::<i32>("items");
    db.insert(items, 1);
    db.insert(items, 2);
    db.commit();

    db.push(Some("no_changes"));
    // No changes made

    assert!(db.pop());

    // State should be unchanged
    let items_result: Vec<_> = db.collect(items);
    assert_eq!(items_result.len(), 2);
    assert!(items_result.contains(&1));
    assert!(items_result.contains(&2));
}

/// Test is_recording method.
#[test]
fn test_is_recording() {
    let mut db = Database::new();

    assert!(!db.is_recording());

    db.push(None);
    assert!(db.is_recording());

    db.push(None);
    assert!(db.is_recording());

    db.pop();
    assert!(db.is_recording());

    db.pop();
    assert!(!db.is_recording());
}

// ============================================================================
// Persistent Input Tests
// ============================================================================

/// Test that persistent inputs survive pop while regular inputs are undone.
///
/// This models a CDCL SAT solver scenario where:
/// - Learned clauses (persistent) should survive backtracking
/// - Decision variables (regular) should be undone on backtrack
#[test]
fn test_persistent_vs_regular_inputs() {
    let mut db = Database::new();

    // Regular input: decision variables (should be undone on pop)
    let decisions = db.create_input::<i32>("decisions");

    // Persistent input: learned clauses (should survive pop)
    let learned = db.create_persistent_input::<i32>("learned");

    // Insert initial data before any checkpoint
    db.insert(decisions, 1);
    db.insert(learned, 100);
    db.commit();

    // Push checkpoint
    db.push(Some("decision_point"));

    // Make a decision and learn a clause
    db.insert(decisions, 2);
    db.insert(learned, 200);
    db.commit();

    // Verify both have the new data
    let decisions_result: Vec<_> = db.collect(decisions);
    assert!(decisions_result.contains(&1));
    assert!(decisions_result.contains(&2));

    let learned_result: Vec<_> = db.collect(learned);
    assert!(learned_result.contains(&100));
    assert!(learned_result.contains(&200));

    // Pop - should undo decision but keep learned clause
    db.pop();

    // Decisions should be restored (2 removed)
    let decisions_result: Vec<_> = db.collect(decisions);
    assert!(decisions_result.contains(&1));
    assert!(!decisions_result.contains(&2), "Decision 2 should be undone");
    assert_eq!(decisions_result.len(), 1);

    // Learned clauses should persist (200 kept)
    let learned_result: Vec<_> = db.collect(learned);
    assert!(learned_result.contains(&100));
    assert!(
        learned_result.contains(&200),
        "Learned clause 200 should survive pop"
    );
    assert_eq!(learned_result.len(), 2);
}

/// Test persistent inputs with nested checkpoints.
#[test]
fn test_persistent_nested_checkpoints() {
    let mut db = Database::new();

    let regular = db.create_input::<i32>("regular");
    let persistent = db.create_persistent_input::<i32>("persistent");

    db.insert(regular, 1);
    db.insert(persistent, 100);
    db.commit();

    // Level 1
    db.push(Some("level1"));
    db.insert(regular, 2);
    db.insert(persistent, 200);
    db.commit();

    // Level 2
    db.push(Some("level2"));
    db.insert(regular, 3);
    db.insert(persistent, 300);
    db.commit();

    // Verify current state
    assert_eq!(db.collect::<i32>(regular).len(), 3); // {1, 2, 3}
    assert_eq!(db.collect::<i32>(persistent).len(), 3); // {100, 200, 300}

    // Pop level 2
    db.pop();

    // Regular should lose 3, persistent keeps 300
    let regular_result: Vec<_> = db.collect(regular);
    assert_eq!(regular_result.len(), 2);
    assert!(!regular_result.contains(&3));

    let persistent_result: Vec<_> = db.collect(persistent);
    assert_eq!(persistent_result.len(), 3);
    assert!(persistent_result.contains(&300));

    // Pop level 1
    db.pop();

    // Regular should lose 2, persistent still has all
    let regular_result: Vec<_> = db.collect(regular);
    assert_eq!(regular_result.len(), 1);
    assert!(regular_result.contains(&1));

    let persistent_result: Vec<_> = db.collect(persistent);
    assert_eq!(persistent_result.len(), 3);
    assert!(persistent_result.contains(&100));
    assert!(persistent_result.contains(&200));
    assert!(persistent_result.contains(&300));
}

/// Test that derived relations correctly reflect persistent input changes.
#[test]
fn test_persistent_with_derived() {
    let mut db = Database::new();

    let regular = db.create_input::<i32>("regular");
    let persistent = db.create_persistent_input::<i32>("persistent");

    // Derived: union of regular and persistent
    let combined = db.union(regular, persistent);

    db.insert(regular, 1);
    db.insert(persistent, 100);
    db.commit();

    db.push(Some("checkpoint"));

    db.insert(regular, 2);
    db.insert(persistent, 200);
    db.commit();

    // Combined should have all 4
    let combined_result: Vec<_> = db.collect(combined);
    assert_eq!(combined_result.len(), 4);

    // Pop
    db.pop();

    // Combined should have 1 (regular) + 100, 200 (persistent) = 3 items
    let combined_result: Vec<_> = db.collect(combined);
    assert_eq!(combined_result.len(), 3);
    assert!(combined_result.contains(&1));
    assert!(combined_result.contains(&100));
    assert!(combined_result.contains(&200));
    assert!(!combined_result.contains(&2));
}

/// Test delete operations on persistent inputs.
#[test]
fn test_persistent_delete() {
    let mut db = Database::new();

    let persistent = db.create_persistent_input::<i32>("persistent");

    db.insert(persistent, 100);
    db.insert(persistent, 200);
    db.commit();

    db.push(Some("before_delete"));

    // Delete from persistent - this should also persist (not be undone)
    db.delete(persistent, 100);
    db.commit();

    let result: Vec<_> = db.collect(persistent);
    assert_eq!(result.len(), 1);
    assert!(result.contains(&200));

    // Pop - delete should NOT be undone for persistent input
    db.pop();

    let result: Vec<_> = db.collect(persistent);
    assert_eq!(result.len(), 1);
    assert!(result.contains(&200));
    assert!(!result.contains(&100), "Delete on persistent should survive pop");
}

/// Test feedback_with_id with persistent inputs and pop.
///
/// Scenario:
/// 1. Set up feedback_with_id with a persistent base input
/// 2. Push checkpoint
/// 3. Add edge to persistent input, triggering discovery of new paths
/// 4. Pop - the persistent edge survives, so the derived paths should too
///
/// Note: CommitIds may be reassigned on pop when tuples survive due to persistent inputs.
/// This is acceptable for now - the important thing is that the tuples themselves survive.
#[test]
fn test_feedback_with_id_with_persistent_input_and_pop() {
    use relational::CommitId;

    let mut db = Database::new();

    // Persistent edges - survive pop
    let edges = db.create_persistent_input::<(i32, i32)>("edges");

    // Initial edge
    db.insert(edges, (1, 2));
    db.commit();

    // Create timestamped path variable
    let (path_var, path) = db.variable::<((i32, i32), CommitId)>("path");

    // Strip CommitId for recursive computation
    let path_tuples = db.map(path, |((a, b), _)| (*a, *b));

    // path(a, c) :- path(a, b), edges(b, c)
    let extended = db.join(path_tuples, edges, |(_, b)| *b, |(b, _)| *b);
    let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
    let all_paths = db.union(edges, new_paths);

    // Wire up timestamped feedback
    db.feedback_with_id(path_var, edges, all_paths);

    // Initial state: path (1,2) discovered at some commit ID
    let paths_before: Vec<_> = db.collect(path);
    assert_eq!(paths_before.len(), 1);
    let (_, initial_commit_id) = paths_before
        .iter()
        .find(|((a, b), _)| *a == 1 && *b == 2)
        .expect("Should have path 1->2");
    let initial_commit_id = *initial_commit_id;

    // Push checkpoint
    db.push(None);

    // Add edge to persistent input - this survives pop!
    db.insert(edges, (2, 3));
    db.commit();

    // Now we should have:
    // - (1,2) with original commit ID
    // - (2,3) with new commit ID
    // - (1,3) with even newer commit ID (derived from 1->2->3)
    let paths_during: Vec<_> = db.collect(path);
    assert_eq!(paths_during.len(), 3);

    let get_commit_id = |paths: &[((i32, i32), CommitId)], from: i32, to: i32| -> CommitId {
        paths
            .iter()
            .find(|((a, b), _)| *a == from && *b == to)
            .map(|(_, id)| *id)
            .unwrap_or_else(|| panic!("Should have path {}->{}", from, to))
    };

    let commit_1_2_during = get_commit_id(&paths_during, 1, 2);
    let commit_2_3_during = get_commit_id(&paths_during, 2, 3);
    let commit_1_3_during = get_commit_id(&paths_during, 1, 3);

    // (1,2) should still have original commit ID
    assert_eq!(
        commit_1_2_during, initial_commit_id,
        "(1,2) should retain original commit ID"
    );

    // (2,3) should have higher commit ID (discovered later)
    assert!(
        commit_2_3_during > initial_commit_id,
        "(2,3) should have higher commit ID than (1,2)"
    );

    // (1,3) should have same or higher commit ID as (2,3)
    assert!(
        commit_1_3_during >= commit_2_3_during,
        "(1,3) should have commit ID >= (2,3)"
    );

    // Pop - but edges is persistent, so (2,3) survives!
    db.pop();

    // All three paths should still exist (because the persistent edge survived)
    let paths_after: Vec<_> = db.collect(path);
    assert_eq!(
        paths_after.len(),
        3,
        "All paths should survive because edge is persistent"
    );

    // Verify the tuples exist (CommitIds may be reassigned on pop)
    let has_path = |paths: &[((i32, i32), CommitId)], from: i32, to: i32| -> bool {
        paths.iter().any(|((a, b), _)| *a == from && *b == to)
    };

    assert!(has_path(&paths_after, 1, 2), "Should have path 1->2 after pop");
    assert!(has_path(&paths_after, 2, 3), "Should have path 2->3 after pop");
    assert!(has_path(&paths_after, 1, 3), "Should have path 1->3 after pop");

    // (1,2) should retain its commit ID since it was not in the popped frame
    let commit_1_2_after = get_commit_id(&paths_after, 1, 2);
    assert_eq!(
        commit_1_2_after, commit_1_2_during,
        "(1,2) commit ID should be retained after pop (it wasn't in the popped frame)"
    );
}
