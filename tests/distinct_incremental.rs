//! Test that distinct handles incremental updates correctly.
//!
//! The distinct operator needs to track when multiplicities cross the 0 boundary.
//! When input state is already updated (NEW state) before the incremental function
//! runs, it must reconstruct OLD state by reversing the changes.

use relational::Database;

/// Test: distinct after map produces correct results.
/// This is the pattern used in CDCL: map extracts clause IDs, distinct deduplicates.
#[test]
fn test_distinct_after_map() {
    let mut db = Database::new();

    // Input relation with (clause_id, literal) pairs
    let clauses = db.create_input::<(i32, i32)>("clauses");

    // Extract clause IDs and deduplicate
    let clause_ids = db.map(clauses, |(cid, _)| *cid);
    let clause_ids_distinct = db.distinct(clause_ids);

    // Add clauses: clause 1 has literals 1 and 2, clause 2 has literal -1
    db.insert(clauses, (1, 1));
    db.insert(clauses, (1, 2)); // Same clause ID, different literal
    db.insert(clauses, (2, -1));
    db.commit();

    let mut result: Vec<_> = db.collect(clause_ids_distinct);
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

    let input = db.create_input::<i32>("input");
    let distinct = db.distinct(input);

    // Initial state: one copy of 1
    db.insert(input, 1);
    db.commit();

    assert_eq!(db.collect::<i32>(distinct), vec![1]);

    // Add another copy of 1 (multiplicity 2) and add 2
    db.insert(input, 1);
    db.insert(input, 2);
    db.commit();

    let mut result: Vec<_> = db.collect(distinct);
    result.sort();

    // distinct should still show 1 (now with mult 2 in input) and 2
    assert_eq!(result, vec![1, 2]);
}

/// Test: distinct correctly handles incremental delete.
/// Verifies that multiplicity tracking works when a tuple goes from positive to 0.
#[test]
fn test_distinct_incremental_delete() {
    let mut db = Database::new();

    let input = db.create_input::<i32>("input");
    let distinct = db.distinct(input);

    // Initial state: two copies of 1, one copy of 2
    db.insert(input, 1);
    db.insert(input, 1);
    db.insert(input, 2);
    db.commit();

    let mut result: Vec<_> = db.collect(distinct);
    result.sort();
    assert_eq!(result, vec![1, 2]);

    // Delete one copy of 1 (still positive) and delete 2 (goes to 0)
    db.delete(input, 1);
    db.delete(input, 2);
    db.commit();

    // 1 should still be present (multiplicity 1), 2 should be gone
    assert_eq!(db.collect::<i32>(distinct), vec![1]);
}

/// Test: distinct with push/pop correctly restores state.
/// This is the core CDCL pattern where we speculatively add assignments
/// and need to backtrack.
#[test]
fn test_distinct_with_push_pop() {
    let mut db = Database::new();

    let input = db.create_input::<i32>("input");
    let distinct = db.distinct(input);

    // Initial state
    db.insert(input, 1);
    db.insert(input, 2);
    db.commit();

    let initial: Vec<_> = db.collect(distinct);
    assert_eq!(initial.len(), 2);

    // Push checkpoint and add more
    db.push(None);
    db.insert(input, 3);
    db.commit();

    let during: Vec<_> = db.collect(distinct);
    assert_eq!(during.len(), 3);

    // Pop should restore to initial state
    db.pop();

    let mut after: Vec<_> = db.collect(distinct);
    after.sort();
    assert_eq!(after, vec![1, 2], "distinct should be restored after pop");
}

/// Test: The exact CDCL pattern - join + map + distinct + difference.
/// This was the pattern that exposed the bug.
#[test]
fn test_cdcl_pattern() {
    let mut db = Database::new();

    // clauses: (clause_id, literal)
    let clauses = db.create_input::<(i32, i32)>("clauses");
    // assigned: literals that are assigned true
    let assigned = db.create_input::<i32>("assigned");

    // Clause literals that are true (satisfied)
    let clause_lit_true = db.join(clauses, assigned, |(_, lit)| *lit, |lit| *lit);
    let satisfied_clauses = db.map(clause_lit_true, |((cid, _), _)| *cid);
    let satisfied_distinct = db.distinct(satisfied_clauses);

    // All clause IDs
    let all_clause_ids = db.map(clauses, |(cid, _)| *cid);
    let all_clause_ids_distinct = db.distinct(all_clause_ids);

    // Clauses that are NOT satisfied
    let unsatisfied = db.difference(all_clause_ids_distinct, satisfied_distinct);

    // Add clauses: (x1) AND (NOT x1) - unsatisfiable
    db.insert(clauses, (1, 1)); // clause 1 contains x1
    db.insert(clauses, (2, -1)); // clause 2 contains NOT x1
    db.commit();

    // Initially no assignments, both clauses unsatisfied
    let mut unsat: Vec<_> = db.collect(unsatisfied);
    unsat.sort();
    assert_eq!(unsat, vec![1, 2], "Both clauses should be unsatisfied initially");

    // Verify all_clause_ids_distinct is correct
    let mut all_ids: Vec<_> = db.collect(all_clause_ids_distinct);
    all_ids.sort();
    assert_eq!(all_ids, vec![1, 2], "Should have clause IDs 1 and 2");

    // Try x1 = true
    db.push(None);
    db.insert(assigned, 1);
    db.commit();

    // Clause 1 satisfied, clause 2 unsatisfied
    let unsat_after_assign: Vec<_> = db.collect(unsatisfied);
    assert_eq!(unsat_after_assign, vec![2], "Only clause 2 should be unsatisfied");

    // Pop and verify restoration
    db.pop();

    let mut unsat_after_pop: Vec<_> = db.collect(unsatisfied);
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

    let input = db.create_input::<i32>("input");
    let distinct = db.distinct(input);

    // Add same value multiple times
    db.insert(input, 1);
    db.insert(input, 1);
    db.insert(input, 1);
    db.commit();

    // Check that distinct output has multiplicity 1
    let mults: Vec<_> = db.iter_with_multiplicity(distinct).collect();
    assert_eq!(mults.len(), 1, "Should have exactly one distinct tuple");

    let (tuple, mult) = mults[0];
    assert_eq!(*tuple, 1);
    assert_eq!(mult.0, 1, "Multiplicity in distinct output should be 1");

    // Add more copies
    db.insert(input, 1);
    db.insert(input, 1);
    db.commit();

    // Multiplicity should still be 1
    let mults_after: Vec<_> = db.iter_with_multiplicity(distinct).collect();
    assert_eq!(mults_after.len(), 1);
    assert_eq!(mults_after[0].1 .0, 1, "Multiplicity should remain 1");
}
