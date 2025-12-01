//! CDCL SAT Solver - reads DIMACS CNF files.

use std::sync::Once;

use anyhow::{Context, Result};
use cdcl::proof::ProofWriter;
use cdcl::{Cnf, Var};
use clap::Parser;
use consume_on_drop::ConsumeOnDrop;
use relational::database::{Graph, GraphHandle};

static SVG_DUMP: Once = Once::new();

fn dump_svg_once(graph: &GraphHandle, path: &str) {
    SVG_DUMP.call_once(|| {
        if let Err(e) = dump_svg(graph, path) {
            eprintln!("Error dumping SVG: {e:?}");
        }
    });
}

fn dump_svg(graph: &Graph, path: &str) -> Result<()> {
    let svg = graph.to_svg()?;
    std::fs::write(path, svg).with_context(|| format!("failed to write SVG file {path}"))?;
    Ok(())
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

    let _dump_on_finish = args.svg_output.map(|svg_path| {
        let graph = solver.graph();
        let path = svg_path.clone();

        ctrlc::set_handler({
            let graph = graph.clone();
            let path = path.clone();
            move || {
                dump_svg_once(&graph, &path);
                std::process::exit(130);
            }
        })
        .expect("Error setting Ctrl-C handler");

        ConsumeOnDrop::new(move || dump_svg_once(&graph, &path))
    });

    if solver.solve_with_proof(proof_writer.as_mut()) {
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
            None => {}
        }
    }
    println!(" 0");
}
