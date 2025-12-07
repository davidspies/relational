//! DIMACS CNF parsing and solving.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};

use anyhow::{Context, Result, bail};
use relational::database::{Database, DatabaseBuilder};

use crate::types::{ClauseId, Lit};
use crate::{Solver, Var};

/// A parsed CNF formula.
#[derive(Debug, Clone)]
pub struct Cnf {
    /// Number of variables declared in the header.
    pub(crate) num_vars: u32,
    /// The clauses (each is a list of literals).
    pub(crate) clauses: Vec<Vec<Lit>>,
}

/// Result of solving a CNF formula.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveResult {
    /// The formula is satisfiable with the given assignment.
    /// Maps each variable to its truth value.
    Satisfiable(HashMap<Var, bool>),
    /// The formula is unsatisfiable.
    Unsatisfiable,
}

impl Cnf {
    pub fn num_vars(&self) -> u32 {
        self.num_vars
    }

    pub fn num_clauses(&self) -> usize {
        self.clauses.len()
    }

    /// Collect all variables that actually appear in the clauses.
    pub fn vars(&self) -> HashSet<Var> {
        self.clauses
            .iter()
            .flat_map(|clause| clause.iter().map(|lit| lit.var()))
            .collect()
    }

    /// Parse a CNF formula from a DIMACS format string.
    pub fn parse(input: &str) -> Result<Self> {
        Self::parse_reader(input.as_bytes())
    }

    /// Parse a CNF formula from a file path.
    pub fn from_file(path: &str) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("cannot open file: {path}"))?;
        Self::parse_reader(BufReader::new(file)).with_context(|| format!("failed to parse: {path}"))
    }

    /// Parse a CNF formula from any reader.
    pub(crate) fn parse_reader<R: Read>(reader: R) -> Result<Self> {
        let reader = BufReader::new(reader);

        let mut num_vars = 0;
        let mut num_clauses = 0;
        let mut clauses = Vec::new();
        let mut current_clause = Vec::new();
        let mut header_seen = false;

        for line in reader.lines() {
            let line = line.context("read error")?;
            let line = line.trim();

            // Skip empty lines and comments (including SATLIB's % marker)
            if line.is_empty() || line.starts_with('c') || line.starts_with('%') {
                continue;
            }

            if line.starts_with('p') {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 4 || parts[1] != "cnf" {
                    bail!("invalid problem line: {line}");
                }
                num_vars = parts[2]
                    .parse()
                    .with_context(|| format!("invalid variable count: {}", parts[2]))?;
                num_clauses = parts[3]
                    .parse()
                    .with_context(|| format!("invalid clause count: {}", parts[3]))?;
                header_seen = true;
                continue;
            }

            if !header_seen {
                bail!("clause before problem line");
            }

            let mut line_has_zero = false;
            for token in line.split_whitespace() {
                let lit: i32 = token
                    .parse()
                    .with_context(|| format!("invalid literal: {token}"))?;
                if lit == 0 {
                    line_has_zero = true;
                    if !current_clause.is_empty() {
                        clauses.push(current_clause);
                        current_clause = Vec::new();
                    }
                } else {
                    current_clause.push(Lit::from_raw(lit));
                }
            }
            // Some SATLIB files omit the trailing 0 on some lines - treat EOL as clause end
            if !line_has_zero && !current_clause.is_empty() {
                clauses.push(current_clause);
                current_clause = Vec::new();
            }
        }

        if !current_clause.is_empty() {
            clauses.push(current_clause);
        }

        if clauses.len() != num_clauses {
            eprintln!(
                "Warning: expected {} clauses, got {}",
                num_clauses,
                clauses.len()
            );
        }

        Ok(Cnf { num_vars, clauses })
    }

    /// Solve the CNF formula.
    pub fn solve(&self) -> SolveResult {
        let vars = self.vars();
        let mut db_builder = DatabaseBuilder::new();
        db_builder.max_iterations = None;
        let mut solver = Solver::new(&mut db_builder, &vars);
        let mut db = db_builder.build();

        for (i, clause) in self.clauses.iter().enumerate() {
            solver.add_clause(&mut db, ClauseId::new((i + 1) as u32), clause);
        }

        if solver.solve(&mut db) {
            let assignment = vars
                .iter()
                .filter_map(|&v| Some((v, solver.value(v)?)))
                .collect();
            SolveResult::Satisfiable(assignment)
        } else {
            SolveResult::Unsatisfiable
        }
    }

    /// Solve and return the database, solver, and variable set (for access to more detailed results).
    pub fn into_solver(self) -> (Database, Solver, HashSet<Var>) {
        let vars = self.vars();
        let mut db_builder = DatabaseBuilder::new();
        db_builder.max_iterations = None;
        let mut solver = Solver::new(&mut db_builder, &vars);
        let mut db = db_builder.build();

        for (i, clause) in self.clauses.iter().enumerate() {
            solver.add_clause(&mut db, ClauseId::new((i + 1) as u32), clause);
        }

        (db, solver, vars)
    }
}

impl SolveResult {
    /// Check if the result is satisfiable.
    pub fn is_sat(&self) -> bool {
        matches!(self, SolveResult::Satisfiable(_))
    }

    /// Check if the result is unsatisfiable.
    pub fn is_unsat(&self) -> bool {
        matches!(self, SolveResult::Unsatisfiable)
    }

    /// Get the assignment if satisfiable.
    pub fn assignment(&self) -> Option<&HashMap<Var, bool>> {
        match self {
            SolveResult::Satisfiable(a) => Some(a),
            SolveResult::Unsatisfiable => None,
        }
    }
}
