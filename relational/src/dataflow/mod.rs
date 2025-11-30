//! Dataflow graph for tracking dependencies and propagating changes.

mod graph;
mod node;
mod traits;

pub use graph::DataflowGraph;
pub use node::{Node, NodeId};
pub use traits::{AnyChanges, AnyCollection};
