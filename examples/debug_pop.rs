use relational::Database;

fn main() {
    let mut db = Database::new();

    // Create input and derived relations
    let input = db.create_input::<i32>("input");
    let doubled = db.map(input, |x| x * 2);
    let count = db.group_count(doubled, |_| ());

    // Insert initial values
    db.insert(input, 1);
    db.insert(input, 2);
    db.commit();

    println!("Initial state:");
    println!("  input = {:?}", db.collect::<i32>(input));
    println!("  doubled = {:?}", db.collect::<i32>(doubled));
    println!("  count = {:?}", db.collect::<((), i64)>(count));

    // Push checkpoint
    db.push(None);

    // Insert more values
    db.insert(input, 3);
    db.commit();

    println!("\nAfter push + insert 3:");
    println!("  input = {:?}", db.collect::<i32>(input));
    println!("  doubled = {:?}", db.collect::<i32>(doubled));
    println!("  count = {:?}", db.collect::<((), i64)>(count));

    // Pop - should restore to before the push
    db.pop();

    println!("\nAfter pop:");
    println!("  input = {:?}", db.collect::<i32>(input));
    println!("  doubled = {:?}", db.collect::<i32>(doubled));
    println!("  count = {:?}", db.collect::<((), i64)>(count));

    // Verify count is correct
    let count_result: Vec<_> = db.collect(count);
    assert_eq!(count_result, vec![((), 2)], "Count should be 2 after pop, got {:?}", count_result);

    println!("\nTest passed!");
}
