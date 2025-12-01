//! Test that distinct handles incremental updates correctly.
//!
//! The distinct operator needs to track when multiplicities cross the 0 boundary.
//! When input state is already updated (NEW state) before the incremental function
//! runs, it must reconstruct OLD state by reversing the changes.

use relational::database::Database;

/// Test: distinct after map produces correct results.
/// This is the pattern used in CDCL: map extracts clause IDs, distinct deduplicates.
#[test]
fn test_distinct_after_map() {
    let mut db = Database::new();

    // Input relation with (clause_id, literal) pairs
    let (mut clauses, clauses_rel) = db.create_input::<(i32, i32)>();

    // Extract clause IDs and deduplicate
    let clause_ids = clauses_rel.map(|(cid, _)| cid);
    let clause_ids_distinct = clause_ids.distinct();
    let out = clause_ids_distinct.boxed().output();

    // Add clauses: clause 1 has literals 1 and 2, clause 2 has literal -1
    clauses.insert((1, 1));
    clauses.insert((1, 2)); // Same clause ID, different literal
    clauses.insert((2, -1));
    db.commit();

    let mut result = out.collect();
    result.sort();

    assert_eq!(
        result,
        vec![1, 2],
        "distinct should return unique clause IDs"
    );
}

/// Test: distinct correctly handles incremental insert.
/// Verifies that multiplicity tracking works when a tuple goes from 0 to positive.
#[test]
fn test_distinct_incremental_insert() {
    let mut db = Database::new();

    let (mut input, input_rel) = db.create_input::<i32>();
    let distinct_rel = input_rel.distinct();
    let out = distinct_rel.boxed().output();

    // Initial state: one copy of 1
    input.insert(1);
    db.commit();

    assert_eq!(out.collect(), vec![1]);

    // Add another copy of 1 (multiplicity 2) and add 2
    input.insert(1);
    input.insert(2);
    db.commit();

    let mut result = out.collect();
    result.sort();

    // distinct should still show 1 (now with mult 2 in input) and 2
    assert_eq!(result, vec![1, 2]);
}

/// Test: distinct correctly handles undos via push/pop.
/// With seen-set semantics, we use push/pop instead of delete.
#[test]
fn test_distinct_incremental_with_pop() {
    let mut db = Database::new();

    let (mut input, input_rel) = db.create_input::<i32>();
    let distinct_rel = input_rel.distinct();
    let out = distinct_rel.boxed().output();

    // Initial state: 1
    input.insert(1);
    db.commit();

    let mut result = out.collect();
    result.sort();
    assert_eq!(result, vec![1]);

    // Push, then add 2
    db.push();
    input.insert(2);
    db.commit();

    let mut result = out.collect();
    result.sort();
    assert_eq!(result, vec![1, 2]);

    // Pop - 2 should be removed, 1 should remain
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    assert_eq!(out.collect(), vec![1]);
}

/// Test: distinct with push/pop correctly restores state.
/// This is the core CDCL pattern where we speculatively add assignments
/// and need to backtrack.
#[test]
fn test_distinct_with_push_pop() {
    let mut db = Database::new();

    let (mut input, input_rel) = db.create_input::<i32>();
    let distinct_rel = input_rel.distinct();
    let out = distinct_rel.boxed().output();

    // Initial state
    input.insert(1);
    input.insert(2);
    db.commit();

    let initial = out.collect();
    assert_eq!(initial.len(), 2);

    // Push checkpoint and add more
    db.push();
    input.insert(3);
    db.commit();

    let during = out.collect();
    assert_eq!(during.len(), 3);

    // Pop should restore to initial state
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    let mut after = out.collect();
    after.sort();
    assert_eq!(after, vec![1, 2], "distinct should be restored after pop");
}

/// Test: The exact CDCL pattern - join + map + distinct + difference.
/// This was the pattern that exposed the bug.
#[test]
fn test_cdcl_pattern() {
    let mut db = Database::new();

    // clauses: (clause_id, literal)
    let (mut clauses, clauses_rel) = db.create_input::<(i32, i32)>();
    // assigned: literals that are assigned true
    let (mut assigned, assigned_rel) = db.create_input::<i32>();

    // Save clauses_rel so we can use it in multiple places
    let saved_clauses = clauses_rel.save();

    // Clause literals that are true (satisfied)
    // Semijoin clauses with assigned literals, keeping the clause id
    let satisfied_clauses = saved_clauses
        .get()
        .swap()
        .semijoin(assigned_rel)
        .map(|(_, cid)| cid);
    let satisfied_distinct = satisfied_clauses.distinct();

    // All clause IDs
    let all_clause_ids = saved_clauses.get().map(|(cid, _)| cid);
    let all_clause_ids_distinct = all_clause_ids.distinct();

    // Clauses that are NOT satisfied
    let unsatisfied = all_clause_ids_distinct.difference(satisfied_distinct);
    let unsatisfied_out = unsatisfied.boxed().output();

    // Add clauses: (x1) AND (NOT x1) - unsatisfiable
    clauses.insert((1, 1)); // clause 1 contains x1
    clauses.insert((2, -1)); // clause 2 contains NOT x1
    db.commit();

    // Initially no assignments, both clauses unsatisfied
    let mut unsat = unsatisfied_out.collect();
    unsat.sort();
    assert_eq!(
        unsat,
        vec![1, 2],
        "Both clauses should be unsatisfied initially"
    );

    // Try x1 = true
    db.push();
    assigned.insert(1);
    db.commit();

    // Clause 1 satisfied, clause 2 unsatisfied
    let unsat_after_assign = unsatisfied_out.collect();
    assert_eq!(
        unsat_after_assign,
        vec![2],
        "Only clause 2 should be unsatisfied"
    );

    // Pop and verify restoration
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    let mut unsat_after_pop = unsatisfied_out.collect();
    unsat_after_pop.sort();
    assert_eq!(
        unsat_after_pop,
        vec![1, 2],
        "Both clauses should be unsatisfied after pop"
    );
}

/// Test: Verify distinct multiplicity is exactly 1 for present tuples.
#[test]
fn test_distinct_multiplicity() {
    let mut db = Database::new();

    let (mut input, input_rel) = db.create_input::<i32>();
    let distinct_rel = input_rel.distinct();
    let out = distinct_rel.boxed().output();

    // Add same value multiple times
    input.insert(1);
    input.insert(1);
    input.insert(1);
    db.commit();

    // Check that distinct output has exactly one tuple
    let result = out.collect();
    assert_eq!(result.len(), 1, "Should have exactly one distinct tuple");
    assert_eq!(result[0], 1);

    // Add more copies
    input.insert(1);
    input.insert(1);
    db.commit();

    // Still exactly one tuple
    let result_after = out.collect();
    assert_eq!(result_after.len(), 1);
    assert_eq!(result_after[0], 1);
}
