//! CDCL SAT Solver - reads DIMACS CNF files.

use std::env;
use std::sync::Once;

use cdcl::{Cnf, Var};
use relational::database::GraphHandle;

static SVG_DUMP: Once = Once::new();

fn dump_svg(graph: &GraphHandle, path: &str) {
    use anyhow::Context;
    SVG_DUMP.call_once(|| {
        let result: anyhow::Result<()> = (|| {
            let svg = graph.to_svg()?;
            std::fs::write(path, svg).context("failed to write SVG file")?;
            Ok(())
        })();
        if let Err(e) = result {
            eprintln!("Error dumping SVG: {e:?}");
        }
    });
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args.len() > 3 {
        eprintln!("Usage: {} <cnf-file> [svg-output]", args[0]);
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
    let mut solver = cnf.into_solver();

    if let Some(svg_path) = args.get(2) {
        let graph = solver.graph();
        let path = svg_path.clone();

        #[cfg(feature = "ctrlc")]
        {
            let graph = graph.clone();
            let path = path.clone();
            ctrlc::set_handler(move || {
                dump_svg(&graph, &path);
                std::process::exit(130);
            })
            .expect("Error setting Ctrl-C handler");
        }

        if solver.solve() {
            println!("s SATISFIABLE");
            print_assignment(&solver, num_vars);
        } else {
            println!("s UNSATISFIABLE");
        }

        dump_svg(&graph, &path);
    } else if solver.solve() {
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
