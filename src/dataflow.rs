//! Dataflow graph for tracking dependencies and propagating changes.

use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::Tuple;
use crate::change::Change;
use crate::collection::Multiset;

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

/// Type-erased collection storage.
pub trait AnyCollection: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn clone_box(&self) -> Box<dyn AnyCollection>;
    fn clear(&mut self);
    fn is_empty(&self) -> bool;
    /// Merge tuples from another collection into this one (union).
    fn merge_from(&mut self, other: &dyn AnyCollection);
    /// Compute the changes needed to go from `old` to `self`.
    /// Returns (new - old) as insertions and (old - new) as deletions.
    fn diff_from(&self, old: &dyn AnyCollection) -> Box<dyn AnyChanges>;
}

impl<T: Tuple + Send + Sync> AnyCollection for Multiset<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn AnyCollection> {
        Box::new(self.clone())
    }

    fn clear(&mut self) {
        Multiset::clear(self);
    }

    fn is_empty(&self) -> bool {
        Multiset::is_empty(self)
    }

    fn merge_from(&mut self, other: &dyn AnyCollection) {
        if let Some(other_coll) = other.as_any().downcast_ref::<Multiset<T>>() {
            for tuple in other_coll.iter() {
                self.insert(tuple.clone());
            }
        }
    }

    fn diff_from(&self, old: &dyn AnyCollection) -> Box<dyn AnyChanges> {
        use crate::change::{Change, Diff};
        let mut changes = Vec::<Change<T>>::new();

        if let Some(old_coll) = old.as_any().downcast_ref::<Multiset<T>>() {
            // Find insertions: tuples in self but not in old (or with higher multiplicity)
            for (tuple, new_mult) in self.iter_with_multiplicity() {
                let old_mult = old_coll.get(tuple);
                let diff = new_mult.0 - old_mult.0;
                if diff > 0 {
                    changes.push(Change::new(tuple.clone(), Diff(diff)));
                }
            }
            // Find deletions: tuples in old but not in self (or with lower multiplicity)
            for (tuple, old_mult) in old_coll.iter_with_multiplicity() {
                let new_mult = self.get(tuple);
                let diff = old_mult.0 - new_mult.0;
                if diff > 0 {
                    changes.push(Change::new(tuple.clone(), Diff(-diff)));
                }
            }
        }

        Box::new(changes)
    }
}

/// Type-erased change batch.
pub trait AnyChanges: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn is_empty(&self) -> bool;
    fn len(&self) -> usize;
    fn clear(&mut self);
    /// Create an empty version of the same type.
    fn clone_empty(&self) -> Box<dyn AnyChanges>;
}

impl<T: Tuple + Send + Sync> AnyChanges for Vec<Change<T>> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn clear(&mut self) {
        Vec::clear(self);
    }

    fn clone_empty(&self) -> Box<dyn AnyChanges> {
        Box::new(Vec::<Change<T>>::new())
    }
}

/// A type-erased operator function.
pub type OperatorFn = Box<
    dyn Fn(&[&dyn Any], &dyn AnyCollection) -> (Box<dyn AnyCollection>, Box<dyn AnyChanges>)
        + Send
        + Sync,
>;

/// A type-erased incremental operator function.
/// Takes: input changes, input states, output state -> output changes
pub type IncrementalOpFn = Box<
    dyn Fn(&[&dyn AnyChanges], &[&dyn AnyCollection], &dyn AnyCollection) -> Box<dyn AnyChanges>
        + Send
        + Sync,
>;

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

    /// Get the current commit ID.
    pub fn commit_id(&self) -> u64 {
        self.commit_id
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
        operator: OperatorFn,
        incremental_op: Option<IncrementalOpFn>,
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
