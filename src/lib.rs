#![deny(unsafe_code)]

//! A relational query engine with differential dataflow semantics.
//!
//! This crate provides:
//! - Differential collections that track changes over time
//! - Relational operators (join, filter, map, etc.)
//! - Feedback loops with fixed-point iteration
//! - Checkpoints for backtracking state
//!
//! # Example: Transitive Closure
//!
//! ```
//! use relational::Database;
//!
//! let mut db = Database::new();
//!
//! // Create input relation for edges
//! let edges = db.create_input::<(i32, i32)>("edges");
//!
//! // Add edges: 1->2->3->4
//! db.insert(edges, (1, 2));
//! db.insert(edges, (2, 3));
//! db.insert(edges, (3, 4));
//!
//! // Create a variable for the recursive computation
//! let (path_var, path) = db.variable::<(i32, i32)>("path");
//!
//! // path(a, c) :- path(a, b), edge(b, c)
//! let extended = db.join(path, edges, |(_, b)| *b, |(b, _)| *b);
//! let new_paths = db.map(extended, |((a, _), (_, c))| (*a, *c));
//!
//! // path = edges ∪ new_paths
//! let all_paths = db.union(edges, new_paths);
//!
//! // Wire up the feedback loop - this automatically runs to fixed point
//! db.feedback(path_var, all_paths);
//!
//! // Collect results (already computed)
//! let paths = db.collect(path);
//! assert!(paths.contains(&(1, 4))); // Can reach 4 from 1
//! ```
//!
//! # Example: Viewing Differential Changes
//!
//! ```
//! use relational::{Database, Diff};
//!
//! let mut db = Database::new();
//! let items = db.create_input::<i32>("items");
//!
//! // Insert some items (each with multiplicity 1)
//! db.insert(items, 10);
//! db.insert(items, 20);
//! db.insert(items, 10); // 10 now has multiplicity 2
//!
//! // View items with their multiplicities
//! for (item, mult) in db.iter_with_multiplicity(items) {
//!     println!("{} appears {} times", item, mult.0);
//! }
//!
//! // Delete one occurrence of 10
//! db.delete(items, 10);
//!
//! // Now 10 has multiplicity 1
//! assert_eq!(db.multiplicity(items, &10), Diff(1));
//! ```
//!
//! # Example: Using Checkpoints
//!
//! ```
//! use relational::Database;
//!
//! let mut db = Database::new();
//! let numbers = db.create_input::<i32>("numbers");
//! let doubled = db.map(numbers, |n| n * 2);
//!
//! // Add initial data
//! db.insert(numbers, 1);
//! db.insert(numbers, 2);
//!
//! // Create a checkpoint
//! let cp = db.checkpoint(Some("before_changes"));
//!
//! // Make some changes
//! db.insert(numbers, 3);
//! db.delete(numbers, 1);
//!
//! // Current state: numbers = {2, 3}, doubled = {4, 6}
//! assert_eq!(db.collect(numbers).len(), 2);
//!
//! // Restore to checkpoint - derived relations are restored,
//! // but manual inputs (numbers) are NOT auto-reverted
//! let info = db.restore(cp).unwrap();
//!
//! // The derived relation 'doubled' was restored to checkpoint state
//! // Manual input 'numbers' was NOT changed (user decides what to keep)
//! // info.manual_input_nodes tells you which inputs weren't restored
//! ```

pub mod cdcl;
mod change;
mod checkpoint;
mod collection;
mod database;
mod dataflow;
pub mod operators;
mod relation;

pub use cdcl::Solver;
pub use change::{Change, Diff};
pub use checkpoint::Checkpoint;
pub use collection::Multiset;
pub use database::{CommitId, Database};
pub use dataflow::{Node, NodeId};
pub use relation::{Relation, Variable};

/// A trait for types that can be used as tuples in relations.
pub trait Tuple: Clone + Eq + std::hash::Hash + std::fmt::Debug + 'static {}
impl<T: Clone + Eq + std::hash::Hash + std::fmt::Debug + 'static> Tuple for T {}
