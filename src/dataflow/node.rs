//! Node types for the dataflow graph.

use super::traits::{AnyChanges, AnyCollection, IncrementalOpFn, OperatorFn};

/// A unique identifier for a node in the dataflow graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) usize);

impl NodeId {
    pub fn index(&self) -> usize {
        self.0
    }
}

/// The kind of a dataflow node.
#[derive(Debug, Clone)]
pub enum NodeKind {
    /// An input relation that receives external data.
    Input { name: String },
    /// A derived relation computed from other nodes.
    Derived { name: Option<String> },
    /// A feedback node for fixed-point iteration.
    Feedback { name: String },
}

/// A node in the dataflow graph.
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// IDs of nodes this node depends on.
    pub inputs: Vec<NodeId>,
    /// The current collection state.
    pub state: Box<dyn AnyCollection>,
    /// Pending changes to be propagated.
    pub pending_changes: Box<dyn AnyChanges>,
    /// The operator to recompute this node (for non-incremental updates).
    pub operator: Option<OperatorFn>,
    /// The incremental operator (if available).
    pub incremental_op: Option<IncrementalOpFn>,
    /// Whether this is a manual input (not auto-reverted on backtrack).
    pub is_manual_input: bool,
    /// Whether this input is persistent (changes survive pop).
    /// Only meaningful for input nodes.
    pub is_persistent: bool,
}

impl Node {
    pub fn name(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Input { name } => Some(name),
            NodeKind::Derived { name } => name.as_deref(),
            NodeKind::Feedback { name } => Some(name),
        }
    }

    pub fn is_input(&self) -> bool {
        matches!(self.kind, NodeKind::Input { .. })
    }

    pub fn is_feedback(&self) -> bool {
        matches!(self.kind, NodeKind::Feedback { .. })
    }

    pub fn is_persistent(&self) -> bool {
        self.is_persistent
    }
}
