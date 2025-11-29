use relational::Database;

fn main() {
    println!("=== Debug: [Push, InsertEdge(0,0), Pop] ===\n");


    let mut db = Database::new();
    let edges = db.create_input::<(i32, i32)>("edges");
    let (path_var, path) = db.variable::<(i32, i32)>("path");
    let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
    let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
    let all_paths = db.union(edges, new_paths);
    db.feedback(path_var, edges, all_paths);

    println!("Initial state:");
    println!("  edges: {:?}", sorted(db.collect(edges)));
    println!("  path: {:?}", sorted(db.collect(path)));

    println!("\n--- Push checkpoint ---");
    db.push(None);

    println!("\n--- Insert (0,0) ---");
    db.insert(edges, (0, 0));
    println!("After insert:");
    println!("  edges: {:?}", sorted(db.collect(edges)));
    println!("  path: {:?}", sorted(db.collect(path)));

    println!("\n--- Pop checkpoint ---");
    let popped = db.pop();
    println!("Pop returned: {}", popped);
    println!("After pop:");
    println!("  edges: {:?}", sorted(db.collect(edges)));
    println!("  path: {:?}", sorted(db.collect(path)));

    println!("\n=== Expected: edges=[], path=[] ===");
}

fn sorted(mut v: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    v.sort();
    v
}
