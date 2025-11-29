// Debug script for partial fixpoint ordering
// Run with: cargo run --example debug_partial_fixpoint (after moving to examples/)
// Or: rustc --edition 2021 -L target/debug/deps debug_partial_fixpoint.rs -o debug_partial_fixpoint

use relational::Database;

fn main() {
    let mut db = Database::new();
    let persistent = db.create_persistent_input::<i32>("persistent");
    let regular = db.create_input::<i32>("regular");
    let (a_var, a_rel) = db.variable::<i32>("a");
    let combined = db.union(persistent, regular);
    let (b_var, b_rel) = db.variable::<i32>("b");
    let a_max = db.max(a_rel);
    let b_val = db.map(a_max, |x| x + 1000);

    db.insert(persistent, 1);
    db.commit();
    db.feedback(a_var, combined);
    db.feedback(b_var, b_val);

    let a_initial: Vec<_> = db.collect(a_rel);
    let b_initial: Vec<_> = db.collect(b_rel);
    println!("Initial: A={:?}, B={:?}", a_initial, b_initial);

    db.push(None);
    db.insert(persistent, 100);
    db.insert(regular, 50);
    db.commit();

    let a_during: Vec<_> = db.collect(a_rel);
    let b_during: Vec<_> = db.collect(b_rel);
    println!("During: A={:?}, B={:?}", a_during, b_during);

    println!("About to pop...");
    db.pop();

    let a_after: Vec<_> = db.collect(a_rel);
    let b_after: Vec<_> = db.collect(b_rel);
    println!("After: A={:?}, B={:?}", a_after, b_after);

    println!("Expected: A=[1, 100], B=[1100]");
}
