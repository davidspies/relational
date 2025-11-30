//! The DataflowGraph that manages nodes and dependencies.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::Tuple;
use crate::change::Change;
use crate::collection::Multiset;

use super::node::{Node, NodeId, NodeKind};

/// The dataflow graph managing all nodes and their dependencies.
pub struct DataflowGraph {
    nodes: Vec<Node>,
    name_to_id: HashMap<String, NodeId>,
    /// Nodes that have pending changes and need propagation.
    dirty_nodes: HashSet<NodeId>,
    /// Topological order for propagation (computed lazily).
    topo_order: Option<Vec<NodeId>>,
    /// Current commit ID (mirrored from Database for use by recompute functions).
    commit_id: u64,
}

impl DataflowGraph {
    pub fn new() -> Self {
        DataflowGraph {
            nodes: Vec::new(),
            name_to_id: HashMap::new(),
            dirty_nodes: HashSet::new(),
            topo_order: None,
            commit_id: 0,
        }
    }

    /// Set the current commit ID (called by Database to sync).
    pub fn set_commit_id(&mut self, id: u64) {
        self.commit_id = id;
    }

    /// Create a new input node.
    pub fn create_input<T: Tuple + Send + Sync>(&mut self, name: &str) -> NodeId {
        self.create_input_internal::<T>(name, false)
    }

    /// Create a new persistent input node.
    /// Changes to persistent inputs are NOT recorded and survive pop().
    pub fn create_persistent_input<T: Tuple + Send + Sync>(&mut self, name: &str) -> NodeId {
        self.create_input_internal::<T>(name, true)
    }

    fn create_input_internal<T: Tuple + Send + Sync>(
        &mut self,
        name: &str,
        persistent: bool,
    ) -> NodeId {
        let id = NodeId(self.nodes.len());
        let node = Node {
            id,
            kind: NodeKind::Input {
                name: name.to_string(),
            },
            inputs: Vec::new(),
            state: Box::new(Multiset::<T>::new()),
            pending_changes: Box::new(Vec::<Change<T>>::new()),
            operator: None,
            incremental_op: None,
            is_manual_input: true,
            is_persistent: persistent,
        };
        self.nodes.push(node);
        self.name_to_id.insert(name.to_string(), id);
        self.topo_order = None;
        id
    }

    /// Create a derived node with the given operator.
    pub fn create_derived<T: Tuple + Send + Sync>(
        &mut self,
        name: Option<&str>,
        inputs: Vec<NodeId>,
        operator: super::traits::OperatorFn,
        incremental_op: Option<super::traits::IncrementalOpFn>,
    ) -> NodeId {
        let id = NodeId(self.nodes.len());
        let node = Node {
            id,
            kind: NodeKind::Derived {
                name: name.map(|s| s.to_string()),
            },
            inputs,
            state: Box::new(Multiset::<T>::new()),
            pending_changes: Box::new(Vec::<Change<T>>::new()),
            operator: Some(operator),
            incremental_op,
            is_manual_input: false,
            is_persistent: false,
        };
        self.nodes.push(node);
        if let Some(n) = name {
            self.name_to_id.insert(n.to_string(), id);
        }
        self.topo_order = None;
        id
    }

    /// Create a feedback node for fixed-point iteration.
    pub fn create_feedback<T: Tuple + Send + Sync>(&mut self, name: &str) -> NodeId {
        let id = NodeId(self.nodes.len());
        let node = Node {
            id,
            kind: NodeKind::Feedback {
                name: name.to_string(),
            },
            inputs: Vec::new(),
            state: Box::new(Multiset::<T>::new()),
            pending_changes: Box::new(Vec::<Change<T>>::new()),
            operator: None,
            incremental_op: None,
            is_manual_input: false,
            is_persistent: false,
        };
        self.nodes.push(node);
        self.name_to_id.insert(name.to_string(), id);
        self.topo_order = None;
        id
    }

    /// Get a node by ID.
    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    /// Get a mutable node by ID.
    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0]
    }

    /// Get a node ID by name.
    pub fn get_id(&self, name: &str) -> Option<NodeId> {
        self.name_to_id.get(name).copied()
    }

    /// Mark a node as having pending changes.
    pub fn mark_dirty(&mut self, id: NodeId) {
        self.dirty_nodes.insert(id);
    }

    /// Compute topological order of nodes.
    /// Feedback nodes are treated as having no incoming edges (to break cycles).
    fn compute_topo_order(&self) -> Vec<NodeId> {
        let n = self.nodes.len();
        let mut in_degree = vec![0usize; n];
        let mut dependents: Vec<Vec<NodeId>> = vec![Vec::new(); n];

        for node in &self.nodes {
            // Skip feedback node inputs to break cycles - feedback nodes get their
            // values set explicitly during fixed-point iteration
            if node.is_feedback() {
                continue;
            }
            for &input in &node.inputs {
                dependents[input.0].push(node.id);
                in_degree[node.id.0] += 1;
            }
        }

        let mut queue: VecDeque<NodeId> = VecDeque::new();
        for (i, &deg) in in_degree.iter().enumerate() {
            if deg == 0 {
                queue.push_back(NodeId(i));
            }
        }

        let mut order = Vec::with_capacity(n);
        while let Some(id) = queue.pop_front() {
            order.push(id);
            for &dep in &dependents[id.0] {
                in_degree[dep.0] -= 1;
                if in_degree[dep.0] == 0 {
                    queue.push_back(dep);
                }
            }
        }

        order
    }

    /// Get or compute topological order.
    pub fn topo_order(&mut self) -> &[NodeId] {
        if self.topo_order.is_none() {
            self.topo_order = Some(self.compute_topo_order());
        }
        self.topo_order.as_ref().unwrap()
    }

    /// Get all node IDs.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        (0..self.nodes.len()).map(NodeId)
    }

    /// Clear all dirty flags.
    pub fn clear_dirty(&mut self) {
        self.dirty_nodes.clear();
    }
}

impl Default for DataflowGraph {
    fn default() -> Self {
        Self::new()
    }
}
