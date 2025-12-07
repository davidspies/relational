//! ASP solver command-line interface.

use std::io::{self, Read};

use asp::{AspSolver, parse_smodels};

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let solver = AspSolver::new(program);
    let answer_sets = solver.solve();

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
