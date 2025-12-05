//! Shadow graph for tracking dataflow structure and element counts.

mod export;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

/// A unique identifier for a node in the dataflow graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

impl NodeId {
    /// Get the raw index (for internal use).
    pub(crate) fn index(self) -> usize {
        self.0
    }
}

/// A node in the dataflow graph.
#[derive(Clone, Debug)]
pub(super) struct Node {
    pub(super) id: NodeId,
    pub(super) name: Option<String>,
    pub(super) op_type: &'static str,
    /// Element counter - None for terminal nodes (output, interrupt).
    pub(super) count: Option<Arc<AtomicUsize>>,
    pub(super) parents: Vec<NodeId>,
}

/// The dataflow graph tracking all relations for a Database.
#[derive(Debug, Default)]
pub struct Graph {
    pub(super) nodes: Vec<Node>,
    /// Feedback edges (source -> target) rendered as dashed arrows.
    pub(super) feedback_edges: Vec<(NodeId, NodeId)>,
}

impl Graph {
    /// Create a new empty graph.
    fn new() -> Self {
        Graph {
            nodes: Vec::new(),
            feedback_edges: Vec::new(),
        }
    }

    /// Add a node to the graph, returning its ID and counter.
    pub(crate) fn add_node(
        &mut self,
        op_type: &'static str,
        parents: Vec<NodeId>,
    ) -> (NodeId, Arc<AtomicUsize>) {
        let id = NodeId(self.nodes.len());
        let count = Arc::new(AtomicUsize::new(0));
        self.nodes.push(Node {
            id,
            name: None,
            op_type,
            count: Some(count.clone()),
            parents,
        });
        (id, count)
    }

    /// Add a terminal node (output, interrupt) without a counter.
    pub(crate) fn add_terminal_node(&mut self, op_type: &'static str, parents: Vec<NodeId>) {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node {
            id,
            name: None,
            op_type,
            count: None,
            parents,
        });
    }

    /// Set the name of a node.
    pub(crate) fn set_name(&mut self, id: NodeId, name: String) {
        if let Some(node) = self.nodes.get_mut(id.index()) {
            node.name = Some(name);
        }
    }

    /// Override the op_type of a node.
    pub(crate) fn set_op_type(&mut self, id: NodeId, op_type: &'static str) {
        if let Some(node) = self.nodes.get_mut(id.index()) {
            node.op_type = op_type;
        }
    }

    /// Add a feedback edge (rendered as dashed arrow).
    pub(crate) fn add_feedback_edge(&mut self, source: NodeId, target: NodeId) {
        self.feedback_edges.push((source, target));
    }
}

/// A mutable handle to a Graph during construction (not thread-safe).
/// Contains Some(Graph) during construction, None after finalization.
pub(crate) type GraphBuilder = Rc<RefCell<Option<Graph>>>;

/// An immutable, thread-safe handle to a Graph after construction.
pub type GraphHandle = Arc<Graph>;

/// Create a new graph builder for the construction phase.
pub(crate) fn new_graph_builder() -> GraphBuilder {
    Rc::new(RefCell::new(Some(Graph::new())))
}

/// Finalize a graph builder into an immutable Arc<Graph>.
/// Takes the graph out of the builder, leaving None behind.
/// After this, any attempt to create relations will panic.
pub(crate) fn finalize_graph(builder: GraphBuilder) -> GraphHandle {
    let graph = builder
        .borrow_mut()
        .take()
        .expect("graph already finalized");
    Arc::new(graph)
}
