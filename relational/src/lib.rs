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
//! use relational::database::{Database, Op, join, map, output, save, union};
//!
//! let mut db = Database::new();
//!
//! // Create input relation for edges
//! let (mut edges_h, edges_rel) = db.create_input::<(i32, i32)>();
//! let mut edges = save(edges_rel);
//!
//! // Create a variable for the recursive computation
//! let (path_var, path_var_rel) = db.create_variable::<(i32, i32)>();
//! let mut path_rel = save(path_var_rel);
//!
//! // path(a, c) :- path(a, b), edge(b, c)
//! let extended = join(path_rel.get(), edges.get(), |(_, b)| *b, |(b, _)| *b);
//! let new_paths = map(extended, |((a, _), (_, c))| (a, c));
//!
//! // path = edges ∪ new_paths
//! let all_paths = union(edges.get(), new_paths);
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
//! use relational::database::{Database, Op, map, output, save};
//!
//! let mut db = Database::new();
//! let (mut numbers_h, numbers_rel) = db.create_input::<i32>();
//! let mut numbers = save(numbers_rel);
//! let doubled = map(numbers.get(), |n| n * 2);
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
