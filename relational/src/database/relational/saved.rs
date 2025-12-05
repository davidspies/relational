//! SavedRelation - for using a relation in multiple places.

use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use contiguous_data::{Diff, Multiset};

use crate::database::commit_id::CommitId;

use super::graph::{GraphBuilder, NodeId};
use super::op::Op;
use super::relation::Relation;

/// Shared state for a saved relation.
struct SavedState<T, R: Op<T>> {
    upstream: Relation<R>,
    /// Separate pending multiset for each consumer.
    consumer_queues: Vec<Rc<RefCell<Multiset<T>>>>,
    /// The commit ID when we last pulled from upstream.
    last_update_commit_id: CommitId,
    /// The node ID of the upstream relation (for graph tracking).
    upstream_node_id: NodeId,
    /// Graph builder for creating new nodes.
    graph: GraphBuilder,
}

impl<T: Clone + Eq + Hash, R: Op<T>> SavedState<T, R> {
    /// Pull changes from upstream and distribute to all consumer queues.
    fn update(&mut self) {
        // Skip pull if commit ID hasn't changed since last update.
        let current = self.upstream.commit_id.get();
        if current == self.last_update_commit_id {
            return;
        }
        self.last_update_commit_id = current;

        let consumer_queues = &self.consumer_queues;
        self.upstream.foreach(|t, diff| {
            for queue in consumer_queues {
                queue.borrow_mut().update(t.clone(), diff);
            }
        });
    }
}

/// A saved relation that can be used in multiple places.
///
/// Call `.get()` to obtain a relation that can be used in the dataflow graph.
/// Each call to `.get()` returns a new consumer of the saved data.
pub struct SavedRelation<T, R: Op<T>> {
    state: Rc<RefCell<SavedState<T, R>>>,
}

impl<T: Eq + Hash, R: Op<T>> SavedRelation<T, R> {
    /// Get a relation handle for this saved relation.
    ///
    /// Each call returns a new consumer. All consumers receive the same changes.
    pub fn get(&self) -> Relation<SavedGetter<T, R>> {
        let queue = Rc::new(RefCell::new(Multiset::new()));
        let state = self.state.borrow();
        let commit_id = state.upstream.commit_id.clone();
        let parent_node = state.upstream_node_id;
        let graph = state.graph.clone();
        drop(state);
        self.state.borrow_mut().consumer_queues.push(queue.clone());
        Relation::new(
            SavedGetter {
                state: self.state.clone(),
                queue,
            },
            commit_id,
            graph,
            "get",
            vec![parent_node],
        )
    }
}

/// A getter for a saved relation - implements Relation.
pub struct SavedGetter<T, R: Op<T>> {
    state: Rc<RefCell<SavedState<T, R>>>,
    queue: Rc<RefCell<Multiset<T>>>,
}

impl<T: Clone + Eq + Hash, R: Op<T>> Op<T> for SavedGetter<T, R> {
    fn foreach(&mut self, mut consumer: impl FnMut(T, Diff)) {
        // First pull from upstream to all queues
        self.state.borrow_mut().update();

        // Then drain our queue
        let mut queue = self.queue.borrow_mut();
        for (t, diff) in queue.drain() {
            consumer(t, diff);
        }
    }
}

impl<R> Relation<R> {
    /// Save this relation for use in multiple places.
    pub fn save<T>(self) -> SavedRelation<T, R>
    where
        T: Eq + Hash,
        R: Op<T>,
    {
        let upstream_node_id = self.node_id;
        let graph = self.graph.clone();
        SavedRelation {
            state: Rc::new(RefCell::new(SavedState {
                upstream: self,
                consumer_queues: Vec::new(),
                last_update_commit_id: CommitId::default(),
                upstream_node_id,
                graph,
            })),
        }
    }
}
