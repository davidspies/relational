use relational::Database;

fn main() {
    // Test case: [DeleteEdge(4, 2), InsertEdge(4, 3), InsertEdge(3, 2), InsertEdge(3, 0),
    //             InsertEdge(0, 0), InsertEdge(3, 2), InsertEdge(0, 2)]

    println!("=== Incremental version ===");
    {
        let mut db = Database::new();
        let edges = db.create_input::<(i32, i32)>("edges");
        let (path_var, path) = db.variable::<(i32, i32)>("path");
        let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
        let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
        let all_paths = db.union(edges, new_paths);
        db.feedback(path_var, edges, all_paths);

        db.delete(edges, (4, 2));
        println!("After delete(4,2): paths={:?}", sorted(db.collect(path)));

        db.insert(edges, (4, 3));
        println!("After insert(4,3): paths={:?}", sorted(db.collect(path)));

        db.insert(edges, (3, 2));
        println!("After insert(3,2): paths={:?}", sorted(db.collect(path)));

        db.insert(edges, (3, 0));
        println!("After insert(3,0): paths={:?}", sorted(db.collect(path)));

        db.insert(edges, (0, 0));
        println!("After insert(0,0): paths={:?}", sorted(db.collect(path)));

        db.insert(edges, (3, 2)); // duplicate
        println!("After insert(3,2) again: paths={:?}", sorted(db.collect(path)));

        db.insert(edges, (0, 2));
        println!("After insert(0,2): paths={:?}", sorted(db.collect(path)));
    }

    println!("\n=== Replay version (fresh DB, same ops) ===");
    {
        let mut db = Database::new();
        let edges = db.create_input::<(i32, i32)>("edges");
        let (path_var, path) = db.variable::<(i32, i32)>("path");
        let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
        let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
        let all_paths = db.union(edges, new_paths);
        db.feedback(path_var, edges, all_paths);

        // Same operations
        db.delete(edges, (4, 2));
        db.insert(edges, (4, 3));
        db.insert(edges, (3, 2));
        db.insert(edges, (3, 0));
        db.insert(edges, (0, 0));
        db.insert(edges, (3, 2));
        db.insert(edges, (0, 2));

        println!("Final paths: {:?}", sorted(db.collect(path)));
    }
}

fn sorted(mut v: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    v.sort();
    v
}
