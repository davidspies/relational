//! ASP solver command-line interface.

use std::io::{self, Read, Write};

use asp::{AspSolver, parse_smodels};
use clap::Parser;

#[derive(Parser)]
#[command(name = "asp", about = "ASP solver using PB constraints")]
struct Args {
    /// Number of solutions to find (0 = all solutions)
    #[arg(short = 'n', default_value = "1")]
    limit: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

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

    solver.solve_streaming(args.limit, |answer_set| {
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
