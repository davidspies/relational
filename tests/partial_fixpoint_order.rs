//! Test that partial_stratified_fixpoint correctly limits which feedbacks run.
//!
//! The bug: run_partial_stratified_fixpoint was running ALL feedbacks instead
//! of only feedbacks up to the specified index. This causes B to fire an extra
//! time during A's correction phase, when it shouldn't.

use relational::Database;

/// Test that demonstrates the partial fixpoint ordering bug.
///
/// Pseudocode:
/// ```
/// A = feedback(persistent_input + regular_input)
/// A_filtered = filter(A, |x| x > 2)
/// B = feedback(A_filtered)
/// C = feedback(join(filter(count(A_filtered), |c| c == 0), B))
/// ```
#[test]
fn test_partial_fixpoint_ordering_bug() {
    let mut db = Database::new();

    // Inputs
    let regular_set = db.create_input::<i32>("regular");
    db.insert(regular_set, 0);
    let regular = db.max(regular_set);
    let persistent_set = db.create_persistent_input::<i32>("persistent");
    db.insert(persistent_set, 0);
    let persistent = db.max(persistent_set);

    // A = feedback(persistent_input + regular_input)
    let (a_var, a_rel) = db.variable::<i32>("a");
    let a_max = db.max(a_rel);
    let a_joined = db.join(persistent, regular, |_| (), |_| ());
    let a_input = db.map(a_joined, |&(x, y)| x + y);

    // A_filtered = filter(A, |x| x > 2), then take max
    let a_filtered = db.filter(a_rel, |&x| x >= 2);
    let a_filtered_max = db.max(a_filtered);

    // B = feedback(A_filtered_max)
    let (b_var, b_rel) = db.variable::<i32>("b");
    let b_max = db.max(b_rel);

    // C = feedback(join(filter(count(A_filtered_max), |c| c == 0), B_max))
    let (c_var, c_rel) = db.variable::<i32>("c");
    let a_filtered_max_count = db.group_count(a_filtered_max, |_| ());
    let a_filtered_empty = db.filter(a_filtered_max_count, |&((), c)| c == 0);
    let c_input = db.join(a_filtered_empty, b_max, |_| (), |_| ());
    let c_mapped = db.map(c_input, |&((_unit, _count), b_val)| b_val);

    // Set up feedbacks in order: A, B, C
    db.feedback(a_var, a_input);
    db.feedback(b_var, a_filtered_max);
    db.feedback(c_var, c_mapped);

    db.commit();

    // Initial state: regular=0, persistent=0, sum=0
    // A_max should be 0, A_filtered is empty (0 not > 2), B is empty
    let a_initial: Vec<_> = db.collect(a_max);
    let b_initial: Vec<_> = db.collect(b_rel);
    let c_initial: Vec<_> = db.collect(c_rel);
    assert_eq!(a_initial, vec![0], "A_max should be 0: {:?}", a_initial);
    assert!(b_initial.is_empty(), "B should be empty initially");
    assert!(c_initial.is_empty(), "C should be empty initially");

    // Push checkpoint
    db.push(None);

    // Add persistent=3, regular=1, now sum = 3+1 = 4 > 2
    db.insert(persistent_set, 3);
    db.insert(regular_set, 1);
    db.commit();

    let a_during: Vec<_> = db.collect(a_max);
    let b_during: Vec<_> = db.collect(b_rel);
    let c_during: Vec<_> = db.collect(c_rel);
    // A_max should be 4 (max of 0, 4)
    assert_eq!(a_during, vec![4], "A_max should be 4: {:?}", a_during);
    // B should have {4} (4 passes filter)
    assert_eq!(b_during, vec![4], "B should have 4: {:?}", b_during);
    // C should be empty (A_filtered is not empty)
    assert!(c_during.is_empty(), "C should be empty: {:?}", c_during);

    // Pop - regular reverts to 0, persistent stays at 3, sum = 3+0 = 3 > 2
    db.pop();

    let a_after: Vec<_> = db.collect(a_max);
    let b_after: Vec<_> = db.collect(b_rel);
    let c_after: Vec<_> = db.collect(c_rel);

    // A_max should be 3 (max of 0, 3)
    assert_eq!(a_after, vec![3], "A_max should be 3: {:?}", a_after);
    // B should have {3} (3 passes filter, correction re-adds it)
    assert_eq!(b_after, vec![3], "B should have 3: {:?}", b_after);
    // C should be empty - if it has anything, B fired while A_filtered was empty (BUG!)
    assert!(
        c_after.is_empty(),
        "C should be empty (B should not fire while A_filtered is empty), but got: {:?}",
        c_after
    );
}
