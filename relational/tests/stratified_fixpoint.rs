//! Tests for stratified fixpoint semantics with multiple feedback loops.
//!
//! These tests verify that feedback loops are processed in declaration order,
//! with each reaching fixpoint before the next is applied.

use relational::database::{Database, output};

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
    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();
    let edges = edges_rel.save();

    // First feedback: transitive closure (reachability)
    // reach(a, b) :- edge(a, b)
    // reach(a, c) :- reach(a, b), edge(b, c)
    let (reach_var, reach_var_rel) = db.create_variable::<(i32, i32)>();
    let reach_rel = reach_var_rel.save();

    let new_reach = reach_rel.get().swap().join_values(edges.get());
    let all_reach = edges.get().union(new_reach);

    // Second feedback: triples (a, b, c) where reach(a, b) and reach(b, c)
    let (extended_var, extended_var_rel) = db.create_variable::<(i32, i32, i32)>();
    let reach_join = reach_rel
        .get()
        .swap()
        .join(reach_rel.get());
    let triples = reach_join.map(|(b, (a, c))| (a, b, c));

    // Set up first feedback (reach)
    let reach_input = edges.get().union(all_reach);
    db.feedback(reach_var, reach_input);

    // Create outputs for reading BEFORE inserting data
    let reach_out = output(reach_rel.get().boxed());
    let extended_out = output(extended_var_rel.boxed());

    // Insert edges: 1 -> 2 -> 3
    edges_h.insert((1, 2));
    edges_h.insert((2, 3));
    db.commit();

    // At this point, reach should be at fixpoint: {(1,2), (2,3), (1,3)}
    let reach_result = reach_out.collect();
    assert!(reach_result.contains(&(1, 2)), "reach should contain (1,2)");
    assert!(reach_result.contains(&(2, 3)), "reach should contain (2,3)");
    assert!(reach_result.contains(&(1, 3)), "reach should contain (1,3)");
    assert_eq!(reach_result.len(), 3, "reach should have exactly 3 pairs");

    // Set up second feedback (extended)
    db.feedback(extended_var, triples);

    // Extended should contain all (a, b, c) where reach(a,b) and reach(b,c)
    // With reach = {(1,2), (2,3), (1,3)}:
    // - reach(1,2) and reach(2,3) -> (1, 2, 3)
    // - reach(1,3) and reach(3,?) -> nothing (3 doesn't reach anything)
    // - reach(2,3) and reach(3,?) -> nothing
    let extended_result = extended_out.collect();
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

    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();
    let edges = edges_rel.save();

    // Set up transitive closure
    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = path_var_rel.save();

    let new_paths = path_rel.get().swap().join_values(edges.get());
    let all_paths = edges.get().union(new_paths);

    let path_input = edges.get().union(all_paths);
    db.feedback(path_var, path_input);

    let path_out = output(path_rel.get().boxed());

    // Initial edges
    edges_h.insert((1, 2));
    edges_h.insert((2, 3));
    db.commit();

    // Check initial state
    let paths = path_out.collect();
    assert_eq!(paths.len(), 3); // (1,2), (2,3), (1,3)
    assert!(paths.contains(&(1, 3)));

    // Add another edge
    edges_h.insert((3, 4));
    db.commit();

    // The transitive closure should update
    let paths = path_out.collect();
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

    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();
    let edges = edges_rel.save();

    // Transitive closure
    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = path_var_rel.save();

    let new_paths = path_rel.get().swap().join_values(edges.get());
    let all_paths = edges.get().union(new_paths);

    db.feedback(path_var, all_paths);

    let path_out = output(path_rel.get().boxed());

    edges_h.insert((1, 2));
    edges_h.insert((2, 3));
    db.commit();

    let paths = path_out.collect();

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
    let (mut facts_h, facts_rel) = db.create_input::<i32>();
    facts_h.insert(1);
    db.commit();

    // Level 1: double the facts
    let (doubled_var, doubled_var_rel) = db.create_variable::<i32>();
    let doubled_saved = doubled_var_rel.save();
    let double_op = facts_rel.map(|x| x * 2);

    // Level 2: triple the doubled values
    let (tripled_var, tripled_var_rel) = db.create_variable::<i32>();
    let tripled_saved = tripled_var_rel.save();
    let triple_op = doubled_saved.get().map(|x| x * 3);

    // Level 3: add 1 to tripled values
    let (plus_one_var, plus_one_var_rel) = db.create_variable::<i32>();
    let plus_one_op = tripled_saved.get().map(|x| x + 1);

    // Set up feedbacks in order
    // doubled = double(facts)
    db.feedback(doubled_var, double_op);

    // Create output for reading
    let doubled_out = output(doubled_saved.get().boxed());

    // After first feedback: doubled = {2}
    let doubled_result = doubled_out.collect();
    assert!(doubled_result.contains(&2), "doubled should contain 2");

    // tripled = triple(doubled)
    db.feedback(tripled_var, triple_op);

    // Create output for reading
    let tripled_out = output(tripled_saved.get().boxed());

    // After second feedback: tripled = {6}
    let tripled_result = tripled_out.collect();
    assert!(tripled_result.contains(&6), "tripled should contain 6");

    // plus_one = plus_one(tripled)
    db.feedback(plus_one_var, plus_one_op);

    // Create output for reading
    let plus_one_out = output(plus_one_var_rel.boxed());

    // After third feedback: plus_one = {7}
    let plus_one_result = plus_one_out.collect();
    assert!(plus_one_result.contains(&7), "plus_one should contain 7");
}

/// Test that a feedback that doesn't change anything doesn't cause infinite loops.
#[test]
fn test_feedback_immediate_fixpoint() {
    let mut db = Database::new();

    let (mut items_h, items_rel) = db.create_input::<i32>();
    items_h.insert(1);
    items_h.insert(2);
    db.commit();

    // Feedback that just passes through the input (identity)
    let (var, var_rel) = db.create_variable::<i32>();

    db.feedback(var, items_rel);

    let var_out = output(var_rel.boxed());
    let result = var_out.collect();

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

    let (mut seeds_h, seeds_rel) = db.create_input::<i32>();

    // Variable V - the shared counter
    let (v_var, v_var_rel) = db.create_variable::<i32>();
    let v_rel = v_var_rel.save();

    // v_max = max(v)
    let v_max = v_rel.get().global_max();

    // a = v_max.map(|x| if x % 100 < 50 { x + 7 } else { x })
    let a = v_max.map(|x| if x % 100 < 50 { x + 7 } else { x });

    // b = v_max.map(|x| if x < 200 { x + 31 } else { x })
    let v_max_for_b = v_rel.get().global_max();
    let b = v_max_for_b.map(|x| if x < 200 { x + 31 } else { x });

    // a_new = a.difference(v) - only the NEW values from A
    let a_new = a.difference(v_rel.get());

    // b_new = b.difference(v) - only the NEW values from B
    let b_new = b.difference(v_rel.get());

    // For feedback, we need: v = v ∪ a_new  and  v = v ∪ b_new
    let v_with_a = v_rel.get().union(a_new);
    let v_with_b = v_rel.get().union(b_new);

    // First feedback (A): v = v ∪ a_new
    let seeds_saved = seeds_rel.save();
    let a_input = seeds_saved.get().union(v_with_a);
    db.feedback(v_var.clone(), a_input);

    // Second feedback (B): v = v ∪ b_new
    db.feedback(v_var.clone(), v_with_b);

    // Create output for reading
    let v_out = output(v_rel.get().boxed());

    // Seed with 0
    seeds_h.insert(0);
    db.commit();

    println!("After commit:");
    let v_result: Vec<_> = v_out.collect();
    println!("  V = {:?}", v_result);

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
    let (mut input_h, input_rel) = db.create_input::<i32>();

    // A: tracks numbers, adds +2 to each (stays in same parity class)
    let (a_var, a_var_rel) = db.create_variable::<i32>();
    let a_rel = a_var_rel.save();
    let a_plus_2 = a_rel.get().map(|x| x + 2);
    let a_filtered = a_plus_2.filter(|x| *x <= 10); // Cap at 10

    // B: takes A's values, adds +1 (switches parity)
    let (b_var, b_var_rel) = db.create_variable::<i32>();
    let b_from_a = a_rel.get().map(|x| x + 1);
    let b_filtered = b_from_a.filter(|x| *x <= 10);

    // Union B back into A's input (so A grows from B's output too)
    let b_rel = b_var_rel.save();
    let input_saved = input_rel.save();
    let a_combined = input_saved.get().union(b_rel.get());
    let a_recursive = a_combined.union(a_filtered);

    // Set up feedbacks first
    db.feedback(a_var, a_recursive);
    db.feedback(b_var, b_filtered);

    // Create outputs for reading
    let a_out = output(a_rel.get().boxed());
    let b_out = output(b_rel.get().boxed());

    // Start with just 1
    input_h.insert(1);
    db.commit();

    // A should have: 1, 3, 5, 7, 9 (starting from 1, adding 2 each time)
    let a_result: Vec<_> = a_out.collect();
    assert!(a_result.contains(&1), "A should contain 1");
    assert!(a_result.contains(&3), "A should contain 3");
    assert!(a_result.contains(&5), "A should contain 5");

    // Now B should have added even numbers to A via the feedback
    // B = A + 1 = {2, 4, 6, 8, 10}
    let b_result: Vec<_> = b_out.collect();
    assert!(b_result.contains(&2), "B should contain 2");
    assert!(b_result.contains(&4), "B should contain 4");

    // And A should now also have even numbers (from B feeding back)
    let a_result_after: Vec<_> = a_out.collect();
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

    let (mut input_h, input_rel) = db.create_input::<i32>();

    // B = input * 2
    let input_saved = input_rel.save();
    let b = input_saved.get().map(|x| x * 2);

    // C = input + 5
    let c = input_saved.get().map(|x| x + 5);

    // D = B union C
    let d = b.union(c);

    // Use a feedback to test that the diamond is computed correctly
    let (d_var, d_var_rel) = db.create_variable::<i32>();
    db.feedback(d_var, d);

    input_h.insert(10);
    db.commit();

    let d_out = output(d_var_rel.boxed());
    let result: Vec<_> = d_out.collect();
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

    let (mut items_h, items_rel) = db.create_input::<i32>();
    let items_saved = items_rel.save();
    let doubled = items_saved.get().map(|x| x * 2);

    // Initial state
    items_h.insert(1);
    items_h.insert(2);
    db.commit();

    let items_out = output(items_saved.get().boxed());
    let doubled_out = output(doubled.boxed());

    assert_eq!(items_out.collect().len(), 2);
    assert_eq!(doubled_out.collect().len(), 2);

    // Push checkpoint
    db.push();
    assert_eq!(db.depth(), 1);

    // Make changes - with seen-set semantics, we can only add
    items_h.insert(3);
    db.commit();

    let items_result = items_out.collect();
    assert_eq!(items_result.len(), 3); // {1, 2, 3}

    let doubled_result = doubled_out.collect();
    assert!(doubled_result.contains(&2)); // 1*2
    assert!(doubled_result.contains(&4)); // 2*2
    assert!(doubled_result.contains(&6)); // 3*2

    // Pop - should restore to before changes
    assert!(db.pop());
    assert_eq!(db.depth(), 0);

    // Check state is restored - 3 should be gone
    let items_result = items_out.collect();
    assert_eq!(items_result.len(), 2);
    assert!(items_result.contains(&1));
    assert!(items_result.contains(&2));

    let doubled_result = doubled_out.collect();
    assert!(doubled_result.contains(&2)); // 1*2
    assert!(doubled_result.contains(&4)); // 2*2
}

/// Test nested push/pop.
#[test]
fn test_push_pop_nested() {
    let mut db = Database::new();

    let (mut items_h, items_rel) = db.create_input::<i32>();

    items_h.insert(1);
    db.commit();

    let items_out = output(items_rel.boxed());

    // First push
    db.push();
    items_h.insert(2);
    db.commit();

    // Second push
    db.push();
    items_h.insert(3);
    db.commit();

    assert_eq!(items_out.collect().len(), 3); // {1, 2, 3}
    assert_eq!(db.depth(), 2);

    // Pop level2 - should remove 3
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");
    assert_eq!(db.depth(), 1);
    let items_result = items_out.collect();
    assert_eq!(items_result.len(), 2);
    assert!(items_result.contains(&1));
    assert!(items_result.contains(&2));
    assert!(!items_result.contains(&3));

    // Pop level1 - should remove 2
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");
    assert_eq!(db.depth(), 0);
    let items_result = items_out.collect();
    assert_eq!(items_result.len(), 1);
    assert!(items_result.contains(&1));
}

/// Test push/pop with transitive closure.
#[test]
fn test_push_pop_with_feedback() {
    let mut db = Database::new();

    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();
    let edges = edges_rel.save();

    // Set up transitive closure
    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = path_var_rel.save();

    let new_paths = path_rel.get().swap().join_values(edges.get());
    let all_paths = edges.get().union(new_paths);

    db.feedback(path_var, all_paths);

    let path_out = output(path_rel.get().boxed());

    // Initial edges: 1 -> 2 -> 3
    edges_h.insert((1, 2));
    edges_h.insert((2, 3));
    db.commit();

    // Initial paths: (1,2), (2,3), (1,3)
    let paths = path_out.collect();
    assert_eq!(paths.len(), 3);
    assert!(paths.contains(&(1, 3)));

    // Push and add more edges
    db.push();

    edges_h.insert((3, 4));
    db.commit();

    // Now paths should include (3,4), (2,4), (1,4)
    let paths = path_out.collect();
    assert_eq!(paths.len(), 6);
    assert!(paths.contains(&(1, 4)));

    // Pop - should restore to 3 paths
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    let paths = path_out.collect();
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
    assert_eq!(db.depth(), 0);
}

/// Test push without changes followed by pop.
#[test]
fn test_push_pop_no_changes() {
    let mut db = Database::new();

    let (mut items_h, items_rel) = db.create_input::<i32>();
    items_h.insert(1);
    items_h.insert(2);
    db.commit();

    let items_out = output(items_rel.boxed());

    db.push();
    // No changes made

    assert!(db.pop());

    // State should be unchanged
    let items_result = items_out.collect();
    assert_eq!(items_result.len(), 2);
    assert!(items_result.contains(&1));
    assert!(items_result.contains(&2));
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
    let (mut decisions_h, decisions_rel) = db.create_input::<i32>();

    // Persistent input: learned clauses (should survive pop)
    let (mut learned_h, learned_rel) = db.create_persistent_input::<i32>();

    // Insert initial data before any checkpoint
    decisions_h.insert(1);
    learned_h.insert(100);
    db.commit();

    let decisions_out = output(decisions_rel.boxed());
    let learned_out = output(learned_rel.boxed());

    // Push checkpoint
    db.push();

    // Make a decision and learn a clause
    decisions_h.insert(2);
    learned_h.insert(200);
    db.commit();

    // Verify both have the new data
    let decisions_result = decisions_out.collect();
    assert!(decisions_result.contains(&1));
    assert!(decisions_result.contains(&2));

    let learned_result = learned_out.collect();
    assert!(learned_result.contains(&100));
    assert!(learned_result.contains(&200));

    // Pop - should undo decision but keep learned clause
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // Decisions should be restored (2 removed)
    let decisions_result = decisions_out.collect();
    assert!(decisions_result.contains(&1));
    assert!(
        !decisions_result.contains(&2),
        "Decision 2 should be undone"
    );
    assert_eq!(decisions_result.len(), 1);

    // Learned clauses should persist (200 kept)
    let learned_result = learned_out.collect();
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

    let (mut regular_h, regular_rel) = db.create_input::<i32>();
    let (mut persistent_h, persistent_rel) = db.create_persistent_input::<i32>();

    regular_h.insert(1);
    persistent_h.insert(100);
    db.commit();

    let regular_out = output(regular_rel.boxed());
    let persistent_out = output(persistent_rel.boxed());

    // Level 1
    db.push();
    regular_h.insert(2);
    persistent_h.insert(200);
    db.commit();

    // Level 2
    db.push();
    regular_h.insert(3);
    persistent_h.insert(300);
    db.commit();

    // Verify current state
    assert_eq!(regular_out.collect().len(), 3); // {1, 2, 3}
    assert_eq!(persistent_out.collect().len(), 3); // {100, 200, 300}

    // Pop level 2
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // Regular should lose 3, persistent keeps 300
    let regular_result = regular_out.collect();
    assert_eq!(regular_result.len(), 2);
    assert!(!regular_result.contains(&3));

    let persistent_result = persistent_out.collect();
    assert_eq!(persistent_result.len(), 3);
    assert!(persistent_result.contains(&300));

    // Pop level 1
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // Regular should lose 2, persistent still has all
    let regular_result = regular_out.collect();
    assert_eq!(regular_result.len(), 1);
    assert!(regular_result.contains(&1));

    let persistent_result = persistent_out.collect();
    assert_eq!(persistent_result.len(), 3);
    assert!(persistent_result.contains(&100));
    assert!(persistent_result.contains(&200));
    assert!(persistent_result.contains(&300));
}

/// Test that derived relations correctly reflect persistent input changes.
#[test]
fn test_persistent_with_derived() {
    let mut db = Database::new();

    let (mut regular_h, regular_rel) = db.create_input::<i32>();
    let (mut persistent_h, persistent_rel) = db.create_persistent_input::<i32>();

    // Derived: union of regular and persistent
    let combined = regular_rel.union(persistent_rel);
    let combined_out = output(combined.boxed());

    regular_h.insert(1);
    persistent_h.insert(100);
    db.commit();

    db.push();

    regular_h.insert(2);
    persistent_h.insert(200);
    db.commit();

    // Combined should have all 4
    let combined_result = combined_out.collect();
    assert_eq!(combined_result.len(), 4);

    // Pop
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // Combined should have 1 (regular) + 100, 200 (persistent) = 3 items
    let combined_result = combined_out.collect();
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

    let (mut persistent_h, persistent_rel) = db.create_persistent_input::<i32>();

    persistent_h.insert(100);
    persistent_h.insert(200);
    db.commit();

    let persistent_out = output(persistent_rel.boxed());

    db.push();

    // Delete from persistent - this should also persist (not be undone)
    persistent_h.delete(100);
    db.commit();

    let result = persistent_out.collect();
    assert_eq!(result.len(), 1);
    assert!(result.contains(&200));

    // Pop - delete should NOT be undone for persistent input
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    let result = persistent_out.collect();
    assert_eq!(result.len(), 1);
    assert!(result.contains(&200));
    assert!(
        !result.contains(&100),
        "Delete on persistent should survive pop"
    );
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
    use relational::database::CommitId;

    let mut db = Database::new();

    // Persistent edges - survive pop
    let (mut edges_h, edges_rel) = db.create_persistent_input::<(i32, i32)>();

    // Create timestamped path variable
    let (path_var, path_var_rel) = db.create_variable::<((i32, i32), CommitId)>();
    let path_rel = path_var_rel.save();

    // Strip CommitId for recursive computation
    let path_tuples = path_rel.get().map(|((a, b), _)| (a, b));

    // path(a, c) :- path(a, b), edges(b, c)
    let edges_saved = edges_rel.save();
    let new_paths = path_tuples.swap().join_values(edges_saved.get());
    let all_paths = edges_saved.get().union(new_paths);

    // Wire up timestamped feedback
    db.feedback_with_id(path_var, all_paths);

    let path_out = output(path_rel.get().boxed());

    // Initial edge
    edges_h.insert((1, 2));
    db.commit();

    // Initial state: path (1,2) discovered at some commit ID
    let paths_before: Vec<_> = path_out.collect();
    assert_eq!(paths_before.len(), 1);
    let (_, initial_commit_id) = paths_before
        .iter()
        .find(|((a, b), _)| *a == 1 && *b == 2)
        .expect("Should have path 1->2");
    let initial_commit_id = *initial_commit_id;

    // Push checkpoint
    db.push();

    // Add edge to persistent input - this survives pop!
    edges_h.insert((2, 3));
    db.commit();

    // Now we should have:
    // - (1,2) with original commit ID
    // - (2,3) with new commit ID
    // - (1,3) with even newer commit ID (derived from 1->2->3)
    let paths_during: Vec<_> = path_out.collect();
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
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // All three paths should still exist (because the persistent edge survived)
    let paths_after: Vec<_> = path_out.collect();
    assert_eq!(
        paths_after.len(),
        3,
        "All paths should survive because edge is persistent"
    );

    // Verify the tuples exist (CommitIds may be reassigned on pop)
    let has_path = |paths: &[((i32, i32), CommitId)], from: i32, to: i32| -> bool {
        paths.iter().any(|((a, b), _)| *a == from && *b == to)
    };

    assert!(
        has_path(&paths_after, 1, 2),
        "Should have path 1->2 after pop"
    );
    assert!(
        has_path(&paths_after, 2, 3),
        "Should have path 2->3 after pop"
    );
    assert!(
        has_path(&paths_after, 1, 3),
        "Should have path 1->3 after pop"
    );

    // (1,2) should retain its commit ID since it was not in the popped frame
    let commit_1_2_after = get_commit_id(&paths_after, 1, 2);
    assert_eq!(
        commit_1_2_after, commit_1_2_during,
        "(1,2) commit ID should be retained after pop (it wasn't in the popped frame)"
    );
}

/// Minimal test case from property test failure: Push, InsertEdge(0,0), Pop
/// After pop, the path variable should be empty.
#[test]
fn test_push_insert_pop_minimal() {
    let mut db = Database::new();
    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();

    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = path_var_rel.save();

    let edges_saved = edges_rel.save();
    let new_paths = path_rel.get().swap().join_values(edges_saved.get());
    let all_paths = edges_saved.get().union(new_paths);
    db.feedback(path_var, all_paths);

    let path_out = output(path_rel.get().boxed());

    // At this point, path should be empty
    assert_eq!(
        path_out.collect().len(),
        0,
        "Should be empty before any inserts"
    );

    db.push();

    edges_h.insert((0, 0));
    db.commit();

    // Now path should have (0, 0)
    let paths = path_out.collect();
    assert_eq!(paths.len(), 1, "Should have 1 path after insert");
    assert!(paths.contains(&(0, 0)));

    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // After pop, path should be empty again
    let paths = path_out.collect();
    assert_eq!(paths.len(), 0, "Should be empty after pop: {:?}", paths);
}

/// Test: Push, insert, Pop
#[test]
fn test_push_insert_pop() {
    let mut db = Database::new();
    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();

    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = path_var_rel.save();

    let edges_saved = edges_rel.save();
    let new_paths = path_rel.get().swap().join_values(edges_saved.get());
    let all_paths = edges_saved.get().union(new_paths);
    db.feedback(path_var, all_paths);

    let path_out = output(path_rel.get().boxed());

    db.push();

    // Insert an edge
    edges_h.insert((0, 4));
    db.commit();

    // Path should have (0, 4)
    let paths = path_out.collect();
    assert_eq!(paths.len(), 1, "Should have 1 path: {:?}", paths);
    assert!(paths.contains(&(0, 4)));

    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // After pop, path should be empty
    let paths = path_out.collect();
    assert_eq!(paths.len(), 0, "Should be empty after pop: {:?}", paths);
}

/// Test: Push, no changes, Pop
/// After pop, the path variable should still be empty.
#[test]
fn test_push_no_changes_pop() {
    let mut db = Database::new();
    let (_edges_h, edges_rel) = db.create_input::<(i32, i32)>();

    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = path_var_rel.save();

    let edges_saved = edges_rel.save();
    let new_paths = path_rel.get().swap().join_values(edges_saved.get());
    let all_paths = edges_saved.get().union(new_paths);
    db.feedback(path_var, all_paths);

    let path_out = output(path_rel.get().boxed());

    // At this point, path should be empty
    assert_eq!(
        path_out.collect().len(),
        0,
        "Should be empty before any changes"
    );

    db.push();

    // No changes in this checkpoint
    db.commit();

    // Still empty
    let paths = path_out.collect();
    assert_eq!(paths.len(), 0, "Should still be empty with no inserts");

    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // After pop, path should still be empty
    let paths = path_out.collect();
    assert_eq!(paths.len(), 0, "Should be empty after pop: {:?}", paths);
}

/// Test regular feedback (not feedback_with_id) with persistent inputs and pop.
/// This tests if the issue is specific to feedback_with_id.
#[test]
fn test_regular_feedback_with_persistent_input_and_pop() {
    let mut db = Database::new();

    // Persistent edges - survive pop
    let (mut edges_h, edges_rel) = db.create_persistent_input::<(i32, i32)>();

    // Create path variable (no CommitId tracking)
    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = path_var_rel.save();

    // path(a, c) :- path(a, b), edges(b, c)
    let edges_saved = edges_rel.save();
    let new_paths = path_rel.get().swap().join_values(edges_saved.get());
    let all_paths = edges_saved.get().union(new_paths);

    db.feedback(path_var, all_paths);

    let path_out = output(path_rel.get().boxed());

    // Initial edge
    edges_h.insert((1, 2));
    db.commit();

    let paths_before: Vec<_> = path_out.collect();
    assert_eq!(paths_before.len(), 1);
    assert!(paths_before.contains(&(1, 2)));

    // Push checkpoint
    db.push();

    // Add edge to persistent input - this survives pop!
    edges_h.insert((2, 3));
    db.commit();

    // Now we should have 3 paths: (1,2), (2,3), (1,3)
    let paths_during: Vec<_> = path_out.collect();
    assert_eq!(paths_during.len(), 3);
    assert!(paths_during.contains(&(1, 2)));
    assert!(paths_during.contains(&(2, 3)));
    assert!(paths_during.contains(&(1, 3)));

    // Pop - but edges is persistent, so (2,3) survives!
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    // All three paths should still exist (because the persistent edge survived)
    let paths_after: Vec<_> = path_out.collect();
    assert_eq!(
        paths_after.len(),
        3,
        "All paths should survive because edge is persistent: {:?}",
        paths_after
    );
    assert!(paths_after.contains(&(1, 2)));
    assert!(paths_after.contains(&(2, 3)));
    assert!(paths_after.contains(&(1, 3)));
}
