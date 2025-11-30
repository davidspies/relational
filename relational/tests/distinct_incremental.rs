//! Test that distinct handles incremental updates correctly.
//!
//! The distinct operator needs to track when multiplicities cross the 0 boundary.
//! When input state is already updated (NEW state) before the incremental function
//! runs, it must reconstruct OLD state by reversing the changes.

use relational::database::{
    Database, Output, Relation, difference, distinct, join, map, output, save,
};

/// Helper to collect output after update.
fn collect_output<T: relational::Tuple + Clone>(out: &mut Output<T>) -> Vec<T> {
    out.collect()
}

/// Test: distinct after map produces correct results.
/// This is the pattern used in CDCL: map extracts clause IDs, distinct deduplicates.
#[test]
fn test_distinct_after_map() {
    let mut db = Database::new();

    // Input relation with (clause_id, literal) pairs
    let (mut clauses, clauses_rel) = db.create_input::<(i32, i32)>();

    // Extract clause IDs and deduplicate
    let clause_ids = map(clauses_rel, |(cid, _)| cid);
    let clause_ids_distinct = distinct(clause_ids);
    let mut out = output(clause_ids_distinct.boxed());

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
    let distinct_rel = distinct(input_rel);
    let mut out = output(distinct_rel.boxed());

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
    let distinct_rel = distinct(input_rel);
    let mut out = output(distinct_rel.boxed());

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
    let distinct_rel = distinct(input_rel);
    let mut out = output(distinct_rel.boxed());

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
    let mut saved_clauses = save(clauses_rel);

    // Clause literals that are true (satisfied)
    let clause_lit_true = join(
        saved_clauses.get(),
        assigned_rel,
        |(_, lit)| *lit,
        |lit| *lit,
    );
    let satisfied_clauses = map(clause_lit_true, |((cid, _), _)| cid);
    let satisfied_distinct = distinct(satisfied_clauses);

    // All clause IDs
    let all_clause_ids = map(saved_clauses.get(), |(cid, _)| cid);
    let all_clause_ids_distinct = distinct(all_clause_ids);

    // Clauses that are NOT satisfied
    let unsatisfied = difference(all_clause_ids_distinct, satisfied_distinct);
    let mut unsatisfied_out = output(unsatisfied.boxed());

    // Add clauses: (x1) AND (NOT x1) - unsatisfiable
    clauses.insert((1, 1)); // clause 1 contains x1
    clauses.insert((2, -1)); // clause 2 contains NOT x1
    db.commit();

    // Initially no assignments, both clauses unsatisfied
    let mut unsat = collect_output(&mut unsatisfied_out);
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
    let unsat_after_assign = collect_output(&mut unsatisfied_out);
    assert_eq!(
        unsat_after_assign,
        vec![2],
        "Only clause 2 should be unsatisfied"
    );

    // Pop and verify restoration
    let popped = db.pop();
    assert!(popped, "Tried to pop past level 0");

    let mut unsat_after_pop = collect_output(&mut unsatisfied_out);
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
    let distinct_rel = distinct(input_rel);
    let mut out = output(distinct_rel.boxed());

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
