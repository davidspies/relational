//! ASP solver command-line interface.

use std::env;
use std::io::{self, Read};

use asp::{AspSolver, parse_smodels};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command-line argument for solution limit (like clingo)
    // Default is 1, pass 0 for all solutions
    let limit: usize = env::args()
        .nth(1)
        .map(|s| s.parse().unwrap_or(1))
        .unwrap_or(1);

    // Read smodels format from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    let program = parse_smodels(&input)?;

    eprintln!(
        "c Parsed program: {} rules, {} symbols, max_atom={}",
        program.rules.len(),
        program.symbols.len(),
        program.max_atom
    );

    let mut solver = AspSolver::new(program);
    let answer_sets = solver.solve_n(limit);

    if answer_sets.is_empty() {
        println!("UNSATISFIABLE");
    } else {
        println!("SATISFIABLE");
        for (i, answer_set) in answer_sets.iter().enumerate() {
            print!("Answer {}: ", i + 1);
            let mut names: Vec<_> = answer_set
                .iter()
                .filter_map(|&atom| solver.atom_name(atom))
                .collect();
            names.sort();
            println!("{}", names.join(" "));
        }
    }

    Ok(())
}
