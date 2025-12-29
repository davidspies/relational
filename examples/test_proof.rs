use std::path::Path;
use roundingsat::{Solver, SolveResult};

fn main() {
    let proof_path = Path::new("test_proof_log");
    
    let mut solver = Solver::new().unwrap();
    solver.set_proof_log(proof_path);
    solver.set_num_vars(2);
    
    // x1 OR x2
    solver.add_clause(&[1, 2]).unwrap();
    // NOT x1
    solver.add_clause(&[-1]).unwrap();
    // NOT x2  
    solver.add_clause(&[-2]).unwrap();
    
    let result = solver.solve();
    println!("Result: {:?}", result);
    
    // Check if proof file was created
    let proof_file = Path::new("test_proof_log.proof");
    if proof_file.exists() {
        println!("Proof file created successfully!");
        let contents = std::fs::read_to_string(proof_file).unwrap();
        println!("First 500 chars of proof:\n{}", &contents[..contents.len().min(500)]);
        std::fs::remove_file(proof_file).unwrap();
    } else {
        println!("ERROR: Proof file was not created");
    }
}
