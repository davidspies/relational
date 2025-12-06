//! DRAT proof logging for UNSAT proofs.
//!
//! DRAT (Deletion Resolution Asymmetric Tautology) is a standard proof format
//! for verifying UNSAT results from SAT solvers.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};

use crate::types::Lit;

/// A proof writer that logs learned clauses in DRAT format.
pub struct ProofWriter {
    writer: BufWriter<File>,
}

impl ProofWriter {
    /// Create a new proof writer that writes to the given file path.
    pub fn new(path: &Path) -> Result<Self> {
        let file = File::create(path)
            .with_context(|| format!("cannot create proof file: {}", path.display()))?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    /// Log a learned clause (addition).
    pub(crate) fn add_clause(&mut self, literals: &[Lit]) -> std::io::Result<()> {
        for lit in literals {
            write!(self.writer, "{} ", lit.raw())?;
        }
        writeln!(self.writer, "0")?;
        Ok(())
    }

    /// Flush the writer to ensure all data is written.
    pub(crate) fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}
