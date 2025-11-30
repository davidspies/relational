//! CDCL SAT Solver - reads DIMACS CNF files.

use std::env;

use cdcl::{Cnf, Var};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <cnf-file>", args[0]);
        std::process::exit(1);
    }

    let cnf = match Cnf::from_file(&args[1]) {
        Ok(cnf) => cnf,
        Err(e) => {
            eprintln!("Error parsing CNF file: {}", e);
            std::process::exit(1);
        }
    };

    let num_vars = cnf.num_vars;
    eprintln!(
        "Parsed {} variables, {} clauses",
        num_vars,
        cnf.clauses.len()
    );

    let mut solver = cnf.into_solver();

    #[cfg(feature = "ctrlc")]
    solver.install_ctrlc_handler();

    if solver.solve() {
        println!("s SATISFIABLE");
        print_assignment(&solver, num_vars);
    } else {
        println!("s UNSATISFIABLE");
    }
}

fn print_assignment(solver: &cdcl::Solver, num_vars: u32) {
    print!("v");
    for v in 1..=num_vars {
        let var = Var::new(v);
        match solver.value(var) {
            Some(true) => print!(" {}", v),
            Some(false) => print!(" -{}", v),
            None => print!(" {}", v), // Unassigned = can be either
        }
    }
    println!(" 0");
}
