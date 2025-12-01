#![deny(unsafe_code)]

//! A relational query engine with differential dataflow semantics.
//!
//! This crate provides:
//! - Differential collections that track changes over time
//! - Relational operators (join, filter, map, etc.)
//! - Feedback loops with fixed-point iteration
//! - Push/pop checkpoints for backtracking state
//!
//! # Example: Transitive Closure
//!
//! ```
//! use relational::database::{Database, Op, output};
//!
//! let mut db = Database::new();
//!
//! // Create input relation for edges
//! let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();
//! let mut edges = edges_rel.save();
//!
//! // Create a variable for the recursive computation
//! let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
//! let mut path_rel = path_var_rel.save();
//!
//! // path(a, c) :- path(a, b), edge(b, c)
//! // Swap path to (b, a), join_values with edges (b, c) -> (a, c)
//! let new_paths = path_rel.get().swap().join_values(edges.get());
//!
//! // path = edges ∪ new_paths
//! let all_paths = edges.get().union(new_paths);
//!
//! // Wire up the feedback loop
//! db.feedback(path_var, all_paths);
//!
//! // Create output before inserting data
//! let mut path_out = output(path_rel.get().boxed());
//!
//! // Add edges: 1->2->3->4
//! edges_h.insert((1, 2));
//! edges_h.insert((2, 3));
//! edges_h.insert((3, 4));
//! db.commit();
//!
//! // Collect results (already computed via fixpoint)
//! let paths = path_out.collect();
//! assert!(paths.contains(&(1, 4))); // Can reach 4 from 1
//! ```
//!
//! # Example: Using Push/Pop Checkpoints
//!
//! ```
//! use relational::database::{Database, Op, output};
//!
//! let mut db = Database::new();
//! let (mut numbers_h, numbers_rel) = db.create_input::<i32>();
//! let mut numbers = numbers_rel.save();
//! let doubled = numbers.get().map(|n| n * 2);
//! let mut doubled_out = output(doubled.boxed());
//! let mut numbers_out = output(numbers.get().boxed());
//!
//! // Add initial data
//! numbers_h.insert(1);
//! numbers_h.insert(2);
//! db.commit();
//!
//! assert_eq!(numbers_out.collect().len(), 2);
//!
//! // Push a checkpoint
//! db.push();
//!
//! // Make some changes
//! numbers_h.insert(3);
//! db.commit();
//!
//! // Current state: numbers = {1, 2, 3}
//! assert_eq!(numbers_out.collect().len(), 3);
//!
//! // Pop to restore to checkpoint
//! db.pop();
//!
//! // State restored: numbers = {1, 2}
//! assert_eq!(numbers_out.collect().len(), 2);
//! ```

mod change;
mod collection;
pub mod database;

pub use change::Diff;
pub use collection::Multiset;
pub use database::{CommitId, Database};

/// Assign a relation to a variable with a name derived from the variable.
///
/// `assign!(x, expr)` expands to `let x = expr.named("x");`
#[macro_export]
macro_rules! assign {
    ($var:ident, $expr:expr) => {
        let $var = $expr.named(stringify!($var));
    };
}

/// Assign a saved relation to a variable with a name derived from the variable.
///
/// `assign_saved!(x, expr)` expands to `let x = expr.named("x").save();`
#[macro_export]
macro_rules! assign_saved {
    ($var:ident, $expr:expr) => {
        let $var = $expr.named(stringify!($var)).save();
    };
}

/// Create an input relation with named handle and relation variables.
///
/// `create_input!(db, handle, rel, Type)` expands to:
/// ```ignore
/// let (handle, rel) = db.create_input::<Type>();
/// let rel = rel.named("rel");
/// ```
#[macro_export]
macro_rules! create_input {
    ($db:expr, $handle:ident, $rel:ident, $ty:ty) => {
        let ($handle, $rel) = $db.create_input::<$ty>();
        let $rel = $rel.named(stringify!($rel));
    };
    ($db:expr, mut $handle:ident, $rel:ident, $ty:ty) => {
        let (mut $handle, $rel) = $db.create_input::<$ty>();
        let $rel = $rel.named(stringify!($rel));
    };
}

/// Create a persistent input relation with named handle and relation variables.
///
/// `create_persistent_input!(db, handle, rel, Type)` expands to:
/// ```ignore
/// let (handle, rel) = db.create_persistent_input::<Type>();
/// let rel = rel.named("rel");
/// ```
#[macro_export]
macro_rules! create_persistent_input {
    ($db:expr, $handle:ident, $rel:ident, $ty:ty) => {
        let ($handle, $rel) = $db.create_persistent_input::<$ty>();
        let $rel = $rel.named(stringify!($rel));
    };
}

/// Create a variable (for feedback loops) with named handle and relation variables.
///
/// `create_variable!(db, var, rel, Type)` expands to:
/// ```ignore
/// let (var, rel) = db.create_variable::<Type>();
/// let rel = rel.named("rel");
/// ```
#[macro_export]
macro_rules! create_variable {
    ($db:expr, $var:ident, $rel:ident, $ty:ty) => {
        let ($var, $rel) = $db.create_variable::<$ty>();
        let $rel = $rel.named(stringify!($rel));
    };
}
