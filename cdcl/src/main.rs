//! CDCL SAT/PB Solver - reads DIMACS CNF and OPB files.

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use std::path::{Path, PathBuf};
use std::sync::Once;

use anyhow::{Context, Result, bail};
use cdcl::proof::ProofWriter;
use cdcl::{Cnf, Opb, Solver, Var};
use clap::Parser;
use consume_on_drop::ConsumeOnDrop;
use contiguous_data::{HashMap, HashSet};
use relational::database::{Database, Graph, GraphHandle};

static SVG_DUMP: Once = Once::new();

fn dump_svg_once(graph: &GraphHandle, path: &Path) {
    SVG_DUMP.call_once(|| {
        if let Err(e) = dump_svg(graph, path) {
            eprintln!("Error dumping SVG: {e:?}");
        }
    });
}

fn dump_svg(graph: &Graph, path: &Path) -> Result<()> {
    let svg = graph.to_svg()?;
    std::fs::write(path, svg)
        .with_context(|| format!("failed to write SVG file {}", path.display()))?;
    Ok(())
}

fn dump_graph_text(graph: &Graph, path: &Path) -> Result<()> {
    let text = graph.to_text();
    std::fs::write(path, text)
        .with_context(|| format!("failed to write graph file {}", path.display()))?;
    Ok(())
}

#[derive(Parser)]
#[command(about = "CDCL SAT/PB Solver")]
struct Args {
    /// Input file (CNF or OPB format, detected by extension)
    input_file: String,

    /// Output SVG file for dataflow graph visualization
    #[arg(long)]
    svg: Option<PathBuf>,

    /// Output text file for dataflow graph (LLM-friendly format)
    #[arg(long)]
    graph: Option<PathBuf>,

    /// Output DRAT proof file (for UNSAT results)
    #[arg(long)]
    proof: Option<PathBuf>,
}

/// Load a problem file and return solver components.
fn load_problem(path: &str) -> Result<(Database, Solver, HashSet<Var>)> {
    if path.ends_with(".opb") {
        let opb = Opb::from_file(path)?;
        Ok(opb.into_solver())
    } else if path.ends_with(".cnf") {
        let cnf = Cnf::from_file(path)?;
        Ok(cnf.into_solver())
    } else {
        bail!("Unknown file extension. Use .cnf for DIMACS CNF or .opb for OPB format.")
    }
}

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let args = Args::parse();

    let (mut db, mut solver, _vars) = load_problem(&args.input_file).unwrap();

    let mut proof_writer = args
        .proof
        .as_ref()
        .map(|path| ProofWriter::new(path).unwrap());

    let _dump_on_finish = args.svg.map(|svg_path| {
        let graph = db.graph();
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

    match solver.solve_with_proof(&mut db, proof_writer.as_mut()) {
        Some(assignment) => {
            println!("s SATISFIABLE");
            print_assignment(&assignment);
        }
        None => {
            println!("s UNSATISFIABLE");
        }
    }

    if let Some(path) = &args.graph
        && let Err(e) = dump_graph_text(&db.graph(), path)
    {
        eprintln!("Error dumping graph: {e:?}");
    }
}

fn print_assignment(assignment: &HashMap<Var, bool>) {
    let mut sorted: Vec<_> = assignment.iter().collect();
    sorted.sort_by_key(|(var, _)| *var);
    print!("v");
    for (var, &value) in sorted {
        if value {
            print!(" {}", var.raw());
        } else {
            print!(" -{}", var.raw());
        }
    }
    println!(" 0");
}
