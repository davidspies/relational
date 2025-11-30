//! Shadow graph for tracking dataflow structure and element counts.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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
    count: Arc<AtomicUsize>,
    parents: Vec<NodeId>,
}

/// The dataflow graph tracking all relations for a Database.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    nodes: Vec<Node>,
}

impl Graph {
    /// Create a new empty graph.
    fn new() -> Self {
        Graph { nodes: Vec::new() }
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
            count: count.clone(),
            parents,
        });
        (id, count)
    }

    /// Set the name of a node.
    pub(crate) fn set_name(&mut self, id: NodeId, name: String) {
        if let Some(node) = self.nodes.get_mut(id.index()) {
            node.name = Some(name);
        }
    }

    /// Export the graph to DOT format for Graphviz.
    pub fn to_dot(&self) -> String {
        let mut out = String::from("digraph dataflow {\n");
        out.push_str("  rankdir=TB;\n");
        out.push_str("  node [shape=box];\n\n");

        for node in &self.nodes {
            let count = node.count.load(Ordering::Relaxed);
            let label = match &node.name {
                Some(name) => format!("{}\\n{}\\n{}", name, node.op_type, count),
                None => format!("{}\\n{}", node.op_type, count),
            };
            out.push_str(&format!("  n{} [label=\"{}\"];\n", node.id.index(), label));
        }

        out.push('\n');

        for node in &self.nodes {
            for parent in &node.parents {
                out.push_str(&format!("  n{} -> n{};\n", parent.index(), node.id.index()));
            }
        }

        out.push_str("}\n");
        out
    }

    /// Export the graph to SVG using the `dot` command.
    /// Returns an error if graphviz is not installed.
    pub fn to_svg(&self) -> Result<String, std::io::Error> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let dot = self.to_dot();
        let mut child = Command::new("dot")
            .args(["-Tsvg"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        child.stdin.as_mut().unwrap().write_all(dot.as_bytes())?;
        let output = child.wait_with_output()?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(std::io::Error::other("dot command failed"))
        }
    }

    /// Dump the graph to stderr (useful for debugging).
    pub fn dump(&self) {
        eprintln!("\n=== Dataflow Graph ===");
        for node in &self.nodes {
            let count = node.count.load(Ordering::Relaxed);
            let name = node.name.as_deref().unwrap_or("(unnamed)");
            eprintln!(
                "  Node {}: {} [{}] - {} elements",
                node.id.index(), name, node.op_type, count
            );
            if !node.parents.is_empty() {
                let parents: Vec<_> = node.parents.iter().map(|p| p.index().to_string()).collect();
                eprintln!("    parents: {}", parents.join(", "));
            }
        }
        eprintln!("======================\n");
    }
}

/// A mutable handle to a Graph during construction (not thread-safe).
pub(crate) type GraphBuilder = Rc<RefCell<Graph>>;

/// An immutable, thread-safe handle to a Graph after construction.
pub type GraphHandle = Arc<Graph>;

/// Create a new graph builder for the construction phase.
pub(crate) fn new_graph_builder() -> GraphBuilder {
    Rc::new(RefCell::new(Graph::new()))
}
