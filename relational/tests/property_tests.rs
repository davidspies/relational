//! Property tests comparing pop() against a naive replay model.
//!
//! The model: instead of using pop(), we replay all operations from scratch,
//! excluding any operations that were inside popped frames.

use proptest::prelude::*;
use relational::database::{Database, Relation, join, map, output, save, union};

/// An operation that can be performed on the database.
#[derive(Debug, Clone)]
enum Op {
    /// Insert an edge (a, b) into the edges input.
    InsertEdge(i32, i32),
    /// Push a new checkpoint frame.
    Push,
    /// Pop the top checkpoint frame (if any).
    Pop,
}

/// Generate a random operation.
fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        // Bias towards inserts to build up interesting state
        5 => (0i32..5, 0i32..5).prop_map(|(a, b)| Op::InsertEdge(a, b)),
        2 => Just(Op::Push),
        2 => Just(Op::Pop),
    ]
}

/// Build a database with transitive closure and apply operations.
/// Returns the final state of the path relation.
fn apply_ops_with_pop(ops: &[Op]) -> Vec<(i32, i32)> {
    let mut db = Database::new();
    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();

    // Set up transitive closure
    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = save(path_var_rel);

    let edges_saved = save(edges_rel);
    let extended = join(path_rel.get(), edges_saved.get(), |(_, b)| *b, |(b, _)| *b);
    let new_paths = map(extended, |((a, _), (_, c))| (a, c));
    let all_paths = union(edges_saved.get(), new_paths);
    db.feedback(path_var, all_paths);

    let path_out = output(path_rel.get().boxed());

    for op in ops {
        match op {
            Op::InsertEdge(a, b) => {
                edges_h.insert((*a, *b));
                db.commit();
            }
            Op::Push => {
                db.push();
            }
            Op::Pop => {
                let _ = db.pop();
            }
        }
    }

    let mut result = path_out.collect();
    result.sort();
    result
}

/// Build a database with transitive closure by replaying only the operations
/// that "survive" the push/pop structure.
///
/// This is the naive model: we track which operations are inside popped frames
/// and simply don't replay them.
fn apply_ops_replay_model(ops: &[Op]) -> Vec<(i32, i32)> {
    // First, figure out which operations survive.
    // We track a stack of "frame start indices" and mark operations as surviving or not.
    let mut surviving = vec![true; ops.len()];
    let mut frame_starts: Vec<usize> = Vec::new();

    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::Push => {
                frame_starts.push(i);
            }
            Op::Pop => {
                if let Some(start) = frame_starts.pop() {
                    // Mark all operations from start+1 to i-1 as not surviving
                    // (the Push and Pop themselves don't matter)
                    for item in surviving.iter_mut().take(i).skip(start + 1) {
                        *item = false;
                    }
                }
                // If no frame to pop, the Pop is a no-op
            }
            _ => {}
        }
    }

    // Now replay only surviving insert/delete operations
    let mut db = Database::new();
    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();

    // Set up transitive closure
    let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
    let path_rel = save(path_var_rel);

    let edges_saved = save(edges_rel);
    let extended = join(path_rel.get(), edges_saved.get(), |(_, b)| *b, |(b, _)| *b);
    let new_paths = map(extended, |((a, _), (_, c))| (a, c));
    let all_paths = union(edges_saved.get(), new_paths);
    db.feedback(path_var, all_paths);

    let path_out = output(path_rel.get().boxed());

    for (i, op) in ops.iter().enumerate() {
        if !surviving[i] {
            continue;
        }
        match op {
            Op::InsertEdge(a, b) => {
                edges_h.insert((*a, *b));
                db.commit();
            }
            Op::Push | Op::Pop => {} // Don't replay push/pop in the model
        }
    }

    let mut result = path_out.collect();
    result.sort();
    result
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Property: pop-based execution matches replay-based execution.
    #[test]
    fn pop_matches_replay_model(ops in prop::collection::vec(arb_op(), 0..30)) {
        let pop_result = apply_ops_with_pop(&ops);
        let replay_result = apply_ops_replay_model(&ops);

        prop_assert_eq!(
            pop_result,
            replay_result,
            "Mismatch for ops: {:?}",
            ops
        );
    }

    /// Property with persistent inputs: pop undoes regular but not persistent.
    #[test]
    fn persistent_inputs_survive_pop(ops in prop::collection::vec(arb_op(), 0..30)) {
        // Use a separate test with persistent inputs
        let (pop_regular, pop_persistent) = apply_ops_with_persistent(&ops);
        let (replay_regular, replay_persistent) = apply_ops_replay_persistent_model(&ops);

        prop_assert_eq!(
            pop_regular,
            replay_regular,
            "Regular input mismatch for ops: {:?}",
            ops
        );

        prop_assert_eq!(
            pop_persistent,
            replay_persistent,
            "Persistent input mismatch for ops: {:?}",
            ops
        );
    }
}

/// Apply operations with both regular and persistent inputs.
fn apply_ops_with_persistent(ops: &[Op]) -> (Vec<i32>, Vec<i32>) {
    let mut db = Database::new();
    let (mut regular_h, regular_rel) = db.create_input::<i32>();
    let (mut persistent_h, persistent_rel) = db.create_persistent_input::<i32>();

    let regular_out = output(regular_rel.boxed());
    let persistent_out = output(persistent_rel.boxed());

    for op in ops {
        match op {
            Op::InsertEdge(a, _) => {
                regular_h.insert(*a);
                persistent_h.insert(*a + 100);
                db.commit();
            }
            Op::Push => {
                db.push();
            }
            Op::Pop => {
                let _ = db.pop();
            }
        }
    }

    let mut regular_result = regular_out.collect();
    let mut persistent_result = persistent_out.collect();
    regular_result.sort();
    persistent_result.sort();
    (regular_result, persistent_result)
}

/// Replay model for persistent inputs.
/// Regular inputs: operations inside popped frames are excluded.
/// Persistent inputs: ALL operations are replayed (nothing is excluded).
fn apply_ops_replay_persistent_model(ops: &[Op]) -> (Vec<i32>, Vec<i32>) {
    // Figure out which operations survive for regular inputs
    let mut surviving = vec![true; ops.len()];
    let mut frame_starts: Vec<usize> = Vec::new();

    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::Push => {
                frame_starts.push(i);
            }
            Op::Pop => {
                if let Some(start) = frame_starts.pop() {
                    for item in surviving.iter_mut().take(i).skip(start + 1) {
                        *item = false;
                    }
                }
            }
            _ => {}
        }
    }

    let mut db = Database::new();
    let (mut regular_h, regular_rel) = db.create_input::<i32>();
    let (mut persistent_h, persistent_rel) = db.create_input::<i32>(); // Use regular input for replay

    let regular_out = output(regular_rel.boxed());
    let persistent_out = output(persistent_rel.boxed());

    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::InsertEdge(a, _) => {
                // Regular: only if surviving
                if surviving[i] {
                    regular_h.insert(*a);
                }
                // Persistent: always
                persistent_h.insert(*a + 100);
                db.commit();
            }
            Op::Push | Op::Pop => {}
        }
    }

    let mut regular_result = regular_out.collect();
    let mut persistent_result = persistent_out.collect();
    regular_result.sort();
    persistent_result.sort();
    (regular_result, persistent_result)
}

/// Additional test: nested push/pop with specific patterns.
#[test]
fn test_nested_pop_specific_case() {
    // A specific case that exercises nested push/pop with feedback
    let ops = vec![
        Op::InsertEdge(1, 2),
        Op::Push,
        Op::InsertEdge(2, 3),
        Op::Push,
        Op::InsertEdge(3, 4),
        Op::Pop,              // Should undo (3,4)
        Op::InsertEdge(2, 4), // This survives
        Op::Pop,              // Should undo (2,3) and (2,4)
    ];

    let pop_result = apply_ops_with_pop(&ops);
    let replay_result = apply_ops_replay_model(&ops);

    assert_eq!(pop_result, replay_result, "Mismatch for nested case");
}

/// Test that exercises multiple feedbacks with pop.
#[test]
fn test_multiple_feedbacks_with_pop() {
    let mut db = Database::new();

    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();

    // First feedback: transitive closure
    let (reach_var, reach_var_rel) = db.create_variable::<(i32, i32)>();
    let reach_rel = save(reach_var_rel);

    let edges_saved = save(edges_rel);
    let extended_reach = join(reach_rel.get(), edges_saved.get(), |(_, b)| *b, |(b, _)| *b);
    let new_reach = map(extended_reach, |((a, _), (_, c))| (a, c));
    let all_reach = union(edges_saved.get(), new_reach);

    // Second feedback: count reachable pairs (self-join on reach)
    let (pairs_var, pairs_var_rel) = db.create_variable::<(i32, i32, i32)>();
    let reach_join = join(reach_rel.get(), reach_rel.get(), |(_, b)| *b, |(b, _)| *b);
    let triples = map(reach_join, |((a, b), (_, c))| (a, b, c));

    // Set up edges: 1 -> 2 -> 3
    edges_h.insert((1, 2));
    edges_h.insert((2, 3));
    db.commit();

    db.feedback(reach_var, all_reach);
    db.feedback(pairs_var, triples);

    let reach_out = output(reach_rel.get().boxed());
    let pairs_out = output(pairs_var_rel.boxed());

    // Initial state
    let reach_before = reach_out.collect();
    let pairs_before = pairs_out.collect();

    // Push and add edge
    db.push();
    edges_h.insert((3, 4));
    db.commit();

    let reach_during = reach_out.collect();
    let _pairs_during = pairs_out.collect();

    // Pop
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    let reach_after = reach_out.collect();
    let pairs_after = pairs_out.collect();

    // reach should be restored
    assert_eq!(
        reach_before.len(),
        reach_after.len(),
        "reach should be restored: before={:?}, after={:?}",
        reach_before,
        reach_after
    );

    // pairs should be restored
    assert_eq!(
        pairs_before.len(),
        pairs_after.len(),
        "pairs should be restored: before={:?}, after={:?}",
        pairs_before,
        pairs_after
    );

    // During should have more
    assert!(
        reach_during.len() > reach_before.len(),
        "reach should grow during: before={}, during={}",
        reach_before.len(),
        reach_during.len()
    );
}
