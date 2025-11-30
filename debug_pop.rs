// Debug script for the failing test case

use std::cell::RefCell;
use std::rc::Rc;

use relational::database2::{
    join, map, save, union, Database2, Relation, Variable, VariableRelation,
};

fn main() {
    // [Push, InsertEdge(1, 3), DeleteEdge(3, 3), InsertEdge(3, 1), Pop]
    let mut db = Database2::new();
    let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();

    // Set up transitive closure
    let path_var = Rc::new(RefCell::new(Variable::<(i32, i32)>::new()));
    let mut path_rel = save(VariableRelation::new(path_var.clone()));

    let mut edges_saved = save(edges_rel);
    let extended = join(
        path_rel.get(),
        edges_saved.get(),
        |(_, b)| *b,
        |(b, _)| *b,
    );
    let new_paths = map(extended, |((a, _), (_, c))| (a, c));
    let all_paths = union(edges_saved.get(), new_paths);
    db.feedback(path_var.clone(), all_paths);

    println!("Initial state:");
    let paths = path_var.borrow().collect();
    println!("  path_var: {:?}", paths);

    // Push
    println!("\n=== Push ===");
    db.push();
    println!("  checkpoint_depth: {}", db.depth());

    // InsertEdge(1, 3)
    println!("\n=== InsertEdge(1, 3) ===");
    edges_h.insert((1, 3));
    db.commit();
    let paths = path_var.borrow().collect();
    println!("  path_var: {:?}", paths);

    // DeleteEdge(3, 3)
    println!("\n=== DeleteEdge(3, 3) ===");
    edges_h.delete((3, 3));
    db.commit();
    let paths = path_var.borrow().collect();
    println!("  path_var: {:?}", paths);
    println!("  input_totals(3,3): {}", path_var.borrow().debug_input_total(&(3, 3)));

    // InsertEdge(3, 1)
    println!("\n=== InsertEdge(3, 1) ===");
    edges_h.insert((3, 1));
    db.commit();
    let paths = path_var.borrow().collect();
    println!("  path_var: {:?}", paths);
    println!("  input_totals(3,3): {}", path_var.borrow().debug_input_total(&(3, 3)));
    println!("  output_state(3,3): {}", path_var.borrow().debug_output(&(3, 3)));

    // Pop
    println!("\n=== Pop ===");
    db.pop();
    let paths = path_var.borrow().collect();
    println!("  path_var: {:?}", paths);
    println!("  input_totals(3,3): {}", path_var.borrow().debug_input_total(&(3, 3)));
    println!("  output_state(3,3): {}", path_var.borrow().debug_output(&(3, 3)));

    println!("\nExpected: []");
    println!("Got: {:?}", paths);
}
