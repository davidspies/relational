use relational::Database;

fn main() {
    let mut db = Database::new();

    // Create relations similar to CDCL
    let clauses = db.create_input::<(i32, i32)>("clauses"); // (clause_id, literal)
    let assigned = db.create_input::<i32>("assigned"); // assigned literals

    // Clause literals that are true (satisfied)
    let clause_lit_true = db.join(clauses, assigned, |(_, lit)| *lit, |lit| *lit);
    let satisfied_clauses = db.map(clause_lit_true, |((cid, _), _)| *cid);
    let satisfied_distinct = db.distinct(satisfied_clauses);

    // All clause IDs
    let all_clause_ids = db.map(clauses, |(cid, _)| *cid);
    let all_clause_ids_distinct = db.distinct(all_clause_ids);

    // Clauses that are NOT satisfied (potential conflicts)
    let unsatisfied = db.difference(all_clause_ids_distinct, satisfied_distinct);

    // Add clauses: (x1) AND (NOT x1)
    // Clause 1: literal 1 (x1)
    // Clause 2: literal -1 (NOT x1)
    db.insert(clauses, (1, 1));  // clause 1 contains x1
    db.insert(clauses, (2, -1)); // clause 2 contains NOT x1
    db.commit();

    println!("Initial state (no assignments):");
    println!("  clauses = {:?}", db.collect::<(i32, i32)>(clauses));
    println!("  assigned = {:?}", db.collect::<i32>(assigned));
    println!("  all_clause_ids = {:?}", db.collect::<i32>(all_clause_ids));
    println!("  all_clause_ids_distinct = {:?}", db.collect::<i32>(all_clause_ids_distinct));
    println!("  satisfied_distinct = {:?}", db.collect::<i32>(satisfied_distinct));
    println!("  unsatisfied = {:?}", db.collect::<i32>(unsatisfied));

    // Push and try assigning x1 = true (literal 1)
    db.push(Some("try x1=true"));
    db.insert(assigned, 1);
    db.commit();

    println!("\nAfter assigning x1=true:");
    println!("  assigned = {:?}", db.collect::<i32>(assigned));
    println!("  satisfied_distinct = {:?}", db.collect::<i32>(satisfied_distinct));
    println!("  unsatisfied = {:?}", db.collect::<i32>(unsatisfied));

    // Clause 1 should be satisfied, clause 2 should be unsatisfied
    let unsat: Vec<_> = db.collect(unsatisfied);
    println!("  Unsatisfied clauses: {:?}", unsat);

    // Pop to backtrack
    db.pop();

    println!("\nAfter pop (backtrack):");
    println!("  assigned = {:?}", db.collect::<i32>(assigned));
    println!("  satisfied_distinct = {:?}", db.collect::<i32>(satisfied_distinct));
    println!("  unsatisfied = {:?}", db.collect::<i32>(unsatisfied));

    // Now try assigning x1 = false (literal -1)
    db.push(Some("try x1=false"));
    db.insert(assigned, -1);
    db.commit();

    println!("\nAfter assigning x1=false:");
    println!("  assigned = {:?}", db.collect::<i32>(assigned));
    println!("  satisfied_distinct = {:?}", db.collect::<i32>(satisfied_distinct));
    println!("  unsatisfied = {:?}", db.collect::<i32>(unsatisfied));

    // Clause 2 should be satisfied, clause 1 should be unsatisfied
    let unsat: Vec<_> = db.collect(unsatisfied);
    println!("  Unsatisfied clauses: {:?}", unsat);

    // Check: with either assignment, one clause is unsatisfied
    // This is correct for UNSAT - can't satisfy both clauses
    println!("\nThis formula is UNSAT - neither assignment satisfies all clauses");
}
