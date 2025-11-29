use relational::Database;

fn main() {
    let mut db = Database::new();

    let seeds = db.create_input::<i32>("seeds");
    let (v_var, v_rel) = db.variable::<i32>("v");

    // v_max = max(v)
    let v_max = db.max(v_rel);

    // a = v_max.map(|x| if x % 100 < 50 { x + 7 } else { x })
    let a = db.map(v_max, |x| if x % 100 < 50 { x + 7 } else { *x });

    // a_new = a.difference(v) - only the NEW values from A
    let a_new = db.difference(a, v_rel);

    // v_with_a = union(v, a_new)
    let v_with_a = db.union(v_rel, a_new);

    // Seed with 0
    db.insert(seeds, 0);
    db.commit();

    println!("Before feedback:");
    println!("  seeds = {:?}", db.collect::<i32>(seeds));
    println!("  V = {:?}", db.collect::<i32>(v_rel));
    println!("  v_max = {:?}", db.collect::<i32>(v_max));
    println!("  a = {:?}", db.collect::<i32>(a));
    println!("  a_new = {:?}", db.collect::<i32>(a_new));
    println!("  v_with_a = {:?}", db.collect::<i32>(v_with_a));

    // First feedback (A): v = v ∪ a_new
    db.feedback(v_var, seeds, v_with_a);

    println!("\nAfter A feedback:");
    println!("  V = {:?}", db.collect::<i32>(v_rel));
    println!("  v_max = {:?}", db.collect::<i32>(v_max));
    println!("  a = {:?}", db.collect::<i32>(a));
    println!("  a_new = {:?}", db.collect::<i32>(a_new));
    println!("  v_with_a = {:?}", db.collect::<i32>(v_with_a));
}
