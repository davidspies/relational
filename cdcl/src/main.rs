//! CDCL SAT Solver - reads DIMACS CNF files.

use std::sync::Once;

use cdcl::proof::ProofWriter;
use cdcl::{Cnf, Var};
use clap::Parser;
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

#[derive(Parser)]
#[command(about = "CDCL SAT Solver")]
struct Args {
    /// Input CNF file in DIMACS format
    cnf_file: String,

    /// Output SVG file for dataflow graph visualization
    svg_output: Option<String>,

    /// Output DRAT proof file (for UNSAT results)
    #[arg(long)]
    proof: Option<String>,
}

fn main() {
    let args = Args::parse();

    let cnf = Cnf::from_file(&args.cnf_file).unwrap();

    let num_vars = cnf.num_vars;
    let mut solver = cnf.into_solver();

    let mut proof_writer = args
        .proof
        .as_ref()
        .map(|path| ProofWriter::new(path).unwrap());

    if let Some(svg_path) = &args.svg_output {
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

        if solver.solve_with_proof(proof_writer.as_mut()) {
            println!("s SATISFIABLE");
            print_assignment(&solver, num_vars);
        } else {
            println!("s UNSATISFIABLE");
        }

        dump_svg(&graph, &path);
    } else if solver.solve_with_proof(proof_writer.as_mut()) {
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
