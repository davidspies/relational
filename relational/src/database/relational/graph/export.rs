//! Export and visualization methods for the dataflow graph.

use super::Graph;
use std::sync::atomic::Ordering;

impl Graph {
    /// Export the graph to a simple text format, easy for LLMs to parse.
    ///
    /// Format: one line per node showing name, op, count, and parent counts.
    /// Example:
    /// ```text
    /// n0: clauses_rel [input] count=7255
    /// n6: [union] count=42107 <- n0(7255), n1(34852)
    /// n7: all_clauses [consolidate] count=42107 <- n6(42107)
    /// ```
    pub fn to_text(&self) -> String {
        let mut out = String::new();

        for node in &self.nodes {
            // Node ID and name
            let name_part = node
                .name
                .as_ref()
                .map(|n| format!("{} ", n))
                .unwrap_or_default();

            // Count
            let count_part = node
                .count
                .as_ref()
                .map(|c| format!(" count={}", c.load(Ordering::Relaxed)))
                .unwrap_or_default();

            // Parents with their counts
            let parents_part = if node.parents.is_empty() {
                String::new()
            } else {
                let parent_strs: Vec<_> = node
                    .parents
                    .iter()
                    .map(|p| {
                        let parent_count = self.nodes[p.index()]
                            .count
                            .as_ref()
                            .map(|c| c.load(Ordering::Relaxed).to_string())
                            .unwrap_or_else(|| "?".to_string());
                        format!("n{}({})", p.index(), parent_count)
                    })
                    .collect();
                format!(" <- {}", parent_strs.join(", "))
            };

            out.push_str(&format!(
                "n{}: {}[{}]{}{}\n",
                node.id.index(),
                name_part,
                node.op_type,
                count_part,
                parents_part
            ));
        }

        // Feedback edges
        if !self.feedback_edges.is_empty() {
            out.push_str("\nFeedback edges:\n");
            for (source, target) in &self.feedback_edges {
                out.push_str(&format!("  n{} -> n{}\n", source.index(), target.index()));
            }
        }

        out
    }

    /// Export the graph to DOT format for Graphviz.
    pub fn to_dot(&self) -> String {
        let mut out = String::from("digraph dataflow {\n");
        out.push_str("  rankdir=TB;\n");
        out.push_str("  node [shape=box];\n\n");

        for node in &self.nodes {
            let label = match (&node.name, &node.count) {
                (Some(name), Some(count)) => {
                    format!(
                        "{}\\n{}\\n{}",
                        name,
                        node.op_type,
                        count.load(Ordering::Relaxed)
                    )
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
                eprintln!("  Node {}: {} [{}]", node.id.index(), name, node.op_type);
            }
            if !node.parents.is_empty() {
                let parents: Vec<_> = node.parents.iter().map(|p| p.index().to_string()).collect();
                eprintln!("    parents: {}", parents.join(", "));
            }
        }
        eprintln!("======================\n");
    }
}
