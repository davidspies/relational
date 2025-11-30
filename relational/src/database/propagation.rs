//! Incremental propagation and recomputation for Database.

use std::collections::{HashMap, HashSet};

use crate::change::Change;
use crate::dataflow::{AnyChanges, NodeId};

use super::Database;

impl Database {
    pub(super) fn recompute_all(&mut self) {
        // Sync commit ID to graph so recompute functions can access it
        self.graph.set_commit_id(self.commit_id.0);

        // Try incremental propagation first - if there are pending changes from
        // inputs or feedback nodes, propagate them through derived nodes.
        if self.propagate_deltas() {
            return;
        }

        // Fall back to full recomputation when there are no pending changes.
        // This handles the case of initial state setup or when derived nodes
        // need to be recomputed from their inputs' current state.
        let topo_order: Vec<NodeId> = self.graph.topo_order().to_vec();

        for &node_id in &topo_order {
            // Skip nodes without a recompute function (inputs, feedback nodes)
            if node_id.index() >= self.recompute_fns.len() {
                continue;
            }
            if let Some(ref recompute_fn) = self.recompute_fns[node_id.index()] {
                let new_state = recompute_fn(&self.graph);
                self.graph.get_mut(node_id).state = new_state;
            }
        }
    }

    /// Incrementally propagate deltas through the dataflow graph.
    /// Returns true if there were changes to propagate, false if nothing to do.
    pub(super) fn propagate_deltas(&mut self) -> bool {
        let topo_order: Vec<NodeId> = self.graph.topo_order().to_vec();

        // Build a map of node_id -> pending changes for this propagation round
        let mut pending: HashMap<NodeId, Box<dyn AnyChanges>> = HashMap::new();

        // First, collect pending changes from all input and feedback nodes
        for &node_id in &topo_order {
            let node = self.graph.get(node_id);
            if (node.is_input() || node.is_feedback()) && !node.pending_changes.is_empty() {
                let changes = self.graph.get_mut(node_id).pending_changes.clone_empty();
                let changes =
                    std::mem::replace(&mut self.graph.get_mut(node_id).pending_changes, changes);
                if !changes.is_empty() {
                    pending.insert(node_id, changes);
                }
            }
        }

        // If no pending changes, nothing to propagate
        if pending.is_empty() {
            return false;
        }

        // Track which nodes have been updated
        let mut updated_nodes: HashSet<NodeId> = pending.keys().copied().collect();

        // Process derived nodes in topological order
        for &node_id in &topo_order {
            let node = self.graph.get(node_id);

            // Skip input and feedback nodes
            if node.is_input() || node.is_feedback() {
                continue;
            }

            // Check if any of this node's inputs were updated
            let inputs = node.inputs.clone();
            let any_input_updated = inputs.iter().any(|id| updated_nodes.contains(id));

            if !any_input_updated {
                continue;
            }

            // Check if we have an incremental function
            let has_incremental = node_id.index() < self.incremental_fns.len()
                && self.incremental_fns[node_id.index()].is_some();

            if has_incremental {
                let incremental_fn = self.incremental_fns[node_id.index()].as_ref().unwrap();

                // Build input changes slice
                let empty_changes: Vec<Box<dyn AnyChanges>> = inputs
                    .iter()
                    .map(|_| Box::new(Vec::<Change<()>>::new()) as Box<dyn AnyChanges>)
                    .collect();

                let input_refs: Vec<&dyn AnyChanges> = inputs
                    .iter()
                    .enumerate()
                    .map(|(i, &input_id)| {
                        pending
                            .get(&input_id)
                            .map(|c| c.as_ref())
                            .unwrap_or(empty_changes[i].as_ref())
                    })
                    .collect();

                // Run the incremental function
                let output_changes = incremental_fn(&self.graph, &input_refs);

                if !output_changes.is_empty() {
                    // Apply changes immediately so downstream nodes see updated state
                    self.apply_changes_to_node(node_id, output_changes.as_ref());
                    pending.insert(node_id, output_changes);
                    updated_nodes.insert(node_id);
                }
            } else {
                // Recompute node - compute from current (updated) input states
                if let Some(ref recompute_fn) = self
                    .recompute_fns
                    .get(node_id.index())
                    .and_then(|f| f.as_ref())
                {
                    let new_state = recompute_fn(&self.graph);
                    let old_state = &self.graph.get(node_id).state;
                    let changes = new_state.diff_from(old_state.as_ref());

                    // Update state immediately
                    self.graph.get_mut(node_id).state = new_state;

                    if !changes.is_empty() {
                        pending.insert(node_id, changes);
                        updated_nodes.insert(node_id);
                    }
                }
            }
        }

        // Clear all pending changes
        for &node_id in &topo_order {
            self.graph.get_mut(node_id).pending_changes.clear();
        }

        true
    }

    /// Apply type-erased changes to a node's state.
    pub(super) fn apply_changes_to_node(&mut self, node_id: NodeId, changes: &dyn AnyChanges) {
        // Use the registered apply function for this node
        if let Some(apply_fn) = self.apply_fns.get(node_id.index()).and_then(|f| f.as_ref()) {
            let node = self.graph.get_mut(node_id);
            apply_fn(node.state.as_mut(), changes);
        }
    }
}
