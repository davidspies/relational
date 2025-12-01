//! Shadow graph for tracking dataflow structure and element counts.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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
struct Node {
    id: NodeId,
    name: Option<String>,
    op_type: &'static str,
    /// Element counter - None for terminal nodes (output, interrupt).
    count: Option<Arc<AtomicUsize>>,
    parents: Vec<NodeId>,
}

/// The dataflow graph tracking all relations for a Database.
#[derive(Debug, Default)]
pub struct Graph {
    nodes: Vec<Node>,
    /// Feedback edges (source -> target) rendered as dashed arrows.
    feedback_edges: Vec<(NodeId, NodeId)>,
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

    /// Export the graph to DOT format for Graphviz.
    pub fn to_dot(&self) -> String {
        let mut out = String::from("digraph dataflow {\n");
        out.push_str("  rankdir=TB;\n");
        out.push_str("  node [shape=box];\n\n");

        for node in &self.nodes {
            let label = match (&node.name, &node.count) {
                (Some(name), Some(count)) => {
                    format!("{}\\n{}\\n{}", name, node.op_type, count.load(Ordering::Relaxed))
                }
                (Some(name), None) => format!("{}\\n{}", name, node.op_type),
                (None, Some(count)) => {
                    format!("{}\\n{}", node.op_type, count.load(Ordering::Relaxed))
                }
                (None, None) => node.op_type.to_string(),
            };
            out.push_str(&format!("  n{} [label=\"{}\"];\n", node.id.index(), label));
        }

        out.push('\n');

        for node in &self.nodes {
            for parent in &node.parents {
                out.push_str(&format!("  n{} -> n{};\n", parent.index(), node.id.index()));
            }
        }

        // Feedback edges (dashed, don't affect layout)
        if !self.feedback_edges.is_empty() {
            out.push('\n');
            for (source, target) in &self.feedback_edges {
                out.push_str(&format!(
                    "  n{} -> n{} [style=dashed, constraint=false];\n",
                    source.index(),
                    target.index()
                ));
            }
        }

        out.push_str("}\n");
        out
    }

    /// Export the graph to SVG using the `dot` command.
    /// Returns an error if graphviz is not installed.
    pub fn to_svg(&self) -> anyhow::Result<String> {
        use anyhow::Context;
        use std::io::Write;
        use std::process::{Command, Stdio};

        let dot = self.to_dot();
        let mut child = Command::new("dot")
            .args(["-Tsvg"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("failed to run 'dot' command (is graphviz installed?)")?;

        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(dot.as_bytes())
            .context("failed to write to dot stdin")?;
        let output = child
            .wait_with_output()
            .context("failed to read dot output")?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            anyhow::bail!("dot command failed with status {}", output.status)
        }
    }

    /// Dump the graph to stderr (useful for debugging).
    pub fn dump(&self) {
        eprintln!("\n=== Dataflow Graph ===");
        for node in &self.nodes {
            let name = node.name.as_deref().unwrap_or("(unnamed)");
            if let Some(count) = &node.count {
                eprintln!(
                    "  Node {}: {} [{}] - {} elements",
                    node.id.index(),
                    name,
                    node.op_type,
                    count.load(Ordering::Relaxed)
                );
            } else {
                eprintln!(
                    "  Node {}: {} [{}]",
                    node.id.index(),
                    name,
                    node.op_type
                );
            }
            if !node.parents.is_empty() {
                let parents: Vec<_> = node.parents.iter().map(|p| p.index().to_string()).collect();
                eprintln!("    parents: {}", parents.join(", "));
            }
        }
        eprintln!("======================\n");
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
