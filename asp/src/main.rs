//! ASP solver command-line interface.

use std::env;
use std::io::{self, Read, Write};

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
    let atom_names = solver.atom_names();
    let mut count = 0usize;

    solver.solve_streaming(limit, |answer_set| {
        if count == 0 {
            println!("SATISFIABLE");
        }
        count += 1;
        print!("Answer {}: ", count);
        let mut names: Vec<_> = answer_set
            .iter()
            .filter_map(|&atom| atom_names.get(&atom).map(|s| s.as_str()))
            .collect();
        names.sort();
        println!("{}", names.join(" "));
        io::stdout().flush().unwrap();
    });

    if count == 0 {
        println!("UNSATISFIABLE");
    }

    Ok(())
}
