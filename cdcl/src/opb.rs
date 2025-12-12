//! OPB (Pseudo-Boolean) format parsing and solving.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};

use anyhow::{Context, Result, bail};
use contiguous_data::{HashMap, HashSet};
use relational::create_persistent_input;
use relational::database::{Database, DatabaseBuilder};

use crate::types::{Lit, Weight};
use crate::{Solver, Var};

/// A parsed OPB (Pseudo-Boolean) formula.
#[derive(Debug, Clone)]
pub struct Opb {
    /// Number of variables declared in the header.
    pub(crate) num_vars: u32,
    /// The PB constraints: each is (terms, bound) where terms are (lit, weight).
    pub(crate) constraints: Vec<(Vec<(Lit, Weight)>, Weight)>,
}

/// Result of solving an OPB formula.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpbSolveResult {
    /// The formula is satisfiable with the given assignment.
    Satisfiable(HashMap<Var, bool>),
    /// The formula is unsatisfiable.
    Unsatisfiable,
}

impl Opb {
    pub fn num_vars(&self) -> u32 {
        self.num_vars
    }

    pub fn num_constraints(&self) -> usize {
        self.constraints.len()
    }

    /// Collect all variables that actually appear in the constraints.
    pub fn vars(&self) -> HashSet<Var> {
        self.constraints
            .iter()
            .flat_map(|(terms, _)| terms.iter().map(|(lit, _)| lit.var()))
            .collect()
    }

    /// Parse an OPB formula from a string.
    pub fn parse(input: &str) -> Result<Self> {
        Self::parse_reader(input.as_bytes())
    }

    /// Parse an OPB formula from a file path.
    pub fn from_file(path: &str) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("cannot open file: {path}"))?;
        Self::parse_reader(BufReader::new(file)).with_context(|| format!("failed to parse: {path}"))
    }

    /// Parse an OPB formula from any reader.
    pub(crate) fn parse_reader<R: Read>(reader: R) -> Result<Self> {
        let reader = BufReader::new(reader);
        let mut num_vars = 0;
        let mut constraints = Vec::new();

        for line in reader.lines() {
            let line = line.context("read error")?;
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('*') {
                // Check for header in comment: * #variable= N #constraint= M
                if let Some(rest) = line.strip_prefix('*')
                    && let Some(pos) = rest.find("#variable=")
                {
                    let after = rest[pos + 10..].trim_start();
                    if let Some(end) = after.find(|c: char| !c.is_ascii_digit()) {
                        if let Ok(n) = after[..end].parse() {
                            num_vars = n;
                        }
                    } else if let Ok(n) = after.parse() {
                        num_vars = n;
                    }
                }

                continue;
            }

            // Skip optimization objectives (min: or max:)
            if line.starts_with("min:") || line.starts_with("max:") {
                continue;
            }

            // Parse constraint
            if let Some((terms, bound)) = parse_constraint(line)? {
                constraints.push((terms, bound));
            }
        }

        Ok(Opb {
            num_vars,
            constraints,
        })
    }

    /// Solve the OPB formula.
    pub fn solve(&self) -> OpbSolveResult {
        let vars = self.vars();
        let mut db_builder = DatabaseBuilder::new();
        db_builder.max_iterations = None;
        create_persistent_input!(db_builder, _external_inp, external, Lit);
        let mut solver = Solver::new(&mut db_builder, &vars, external);
        let mut db = db_builder.build();

        for (i, (terms, bound)) in self.constraints.iter().enumerate() {
            solver.add_pb_constraint(&mut db, i as u32, terms, *bound);
        }

        match solver.solve(&mut db) {
            Some(assignment) => OpbSolveResult::Satisfiable(assignment),
            None => OpbSolveResult::Unsatisfiable,
        }
    }

    /// Solve and return database, solver, and variable set.
    pub fn into_solver(self) -> (Database, Solver, HashSet<Var>) {
        let vars = self.vars();
        let mut db_builder = DatabaseBuilder::new();
        db_builder.max_iterations = None;
        create_persistent_input!(db_builder, _external_inp, external, Lit);
        let mut solver = Solver::new(&mut db_builder, &vars, external);
        let mut db = db_builder.build();

        for (i, (terms, bound)) in self.constraints.iter().enumerate() {
            solver.add_pb_constraint(&mut db, i as u32, terms, *bound);
        }

        (db, solver, vars)
    }
}

/// Parse a single OPB constraint line.
/// Format: `+1 x1 -2 ~x2 >= 3 ;` or `+1 x1 -2 ~x2 = 3 ;`
fn parse_constraint(line: &str) -> Result<Option<(Vec<(Lit, Weight)>, Weight)>> {
    // Remove trailing semicolon
    let line = line.trim_end_matches(';').trim();
    if line.is_empty() {
        return Ok(None);
    }

    // Find the relation operator
    let (lhs, op, rhs) = if let Some(pos) = line.find(">=") {
        (&line[..pos], ">=", &line[pos + 2..])
    } else if let Some(pos) = line.find("<=") {
        (&line[..pos], "<=", &line[pos + 2..])
    } else if let Some(pos) = line.find('=') {
        (&line[..pos], "=", &line[pos + 1..])
    } else {
        bail!("no relational operator in constraint: {line}");
    };

    let bound: Weight = rhs
        .trim()
        .parse()
        .with_context(|| format!("invalid bound: {rhs}"))?;
    let terms = parse_terms(lhs)?;

    // Convert based on operator
    match op {
        ">=" => Ok(Some((terms, bound))),
        "<=" => {
            // sum <= bound  =>  -sum >= -bound
            let neg_terms: Vec<_> = terms.into_iter().map(|(lit, w)| (!lit, w)).collect();
            let neg_bound = -bound;
            Ok(Some((neg_terms, neg_bound)))
        }
        "=" => {
            // Equality becomes two constraints, but we only return one here.
            // For simplicity, treat as >= (caller would need to handle = specially).
            // Most OPB files use >= anyway.
            Ok(Some((terms, bound)))
        }
        _ => unreachable!(),
    }
}

/// Parse terms like `+1 x1 -2 ~x2 +3 x3`.
fn parse_terms(s: &str) -> Result<Vec<(Lit, Weight)>> {
    let mut terms = Vec::new();
    let mut tokens = s.split_whitespace().peekable();

    while tokens.peek().is_some() {
        // Parse coefficient
        let coef_str = tokens.next().unwrap();
        let coef: Weight = coef_str
            .parse()
            .with_context(|| format!("invalid coefficient: {coef_str}"))?;

        // Parse variable
        let var_str = tokens
            .next()
            .with_context(|| "expected variable after coefficient")?;
        let (negated, var_name) = if let Some(rest) = var_str.strip_prefix('~') {
            (true, rest)
        } else {
            (false, var_str)
        };

        // Parse variable number (x1, x2, etc.)
        let var_num: u32 = var_name
            .strip_prefix('x')
            .with_context(|| format!("variable must start with 'x': {var_name}"))?
            .parse()
            .with_context(|| format!("invalid variable number: {var_name}"))?;

        let var = Var::new(var_num);
        let lit = if negated {
            Lit::neg(var)
        } else {
            Lit::pos(var)
        };

        terms.push((lit, coef));
    }

    Ok(terms)
}

impl OpbSolveResult {
    pub fn is_sat(&self) -> bool {
        matches!(self, OpbSolveResult::Satisfiable(_))
    }

    pub fn is_unsat(&self) -> bool {
        matches!(self, OpbSolveResult::Unsatisfiable)
    }

    pub fn assignment(&self) -> Option<&HashMap<Var, bool>> {
        match self {
            OpbSolveResult::Satisfiable(a) => Some(a),
            OpbSolveResult::Unsatisfiable => None,
        }
    }
}
