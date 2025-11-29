//! Dataflow graph for tracking dependencies and propagating changes.

mod node;
mod traits;
mod graph;

pub use node::{Node, NodeId, NodeKind};
pub use traits::{AnyChanges, AnyCollection, IncrementalOpFn, OperatorFn};
pub use graph::DataflowGraph;
