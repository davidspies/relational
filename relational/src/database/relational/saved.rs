//! SavedRelation - for using a relation in multiple places.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::Tuple;
use crate::change::{Change, Diff};
use crate::collection::Multiset;
use crate::database::commit_id::CommitId;

use super::relation::Relation;

/// Shared state for a saved relation.
struct SavedState<T: Tuple, R: Relation<T>> {
    upstream: R,
    /// Separate pending multiset for each consumer.
    consumer_queues: Vec<Rc<RefCell<Multiset<T>>>>,
    /// Optional commit ID tracker for optimization.
    commit_id: Option<Rc<Cell<CommitId>>>,
    /// The commit ID when we last pulled from upstream.
    last_update_commit_id: CommitId,
}

impl<T: Tuple + 'static, R: Relation<T>> SavedState<T, R> {
    /// Pull changes from upstream and distribute to all consumer queues.
    fn update(&mut self) {
        // Optimization: skip pull if commit ID hasn't changed since last update
        if let Some(ref commit_id) = self.commit_id {
            let current = commit_id.get();
            if current == self.last_update_commit_id {
                return;
            }
            self.last_update_commit_id = current;
        }

        self.upstream.foreach(&mut |t, diff| {
            for queue in &self.consumer_queues {
                queue
                    .borrow_mut()
                    .apply_change(Change::new(t.clone(), diff));
            }
        });
    }
}

/// A saved relation that can be used in multiple places.
///
/// Call `.get()` to obtain a relation that can be used in the dataflow graph.
/// Each call to `.get()` returns a new consumer of the saved data.
pub struct SavedRelation<T: Tuple, R: Relation<T>> {
    state: Rc<RefCell<SavedState<T, R>>>,
}

impl<T: Tuple + 'static, R: Relation<T>> SavedRelation<T, R> {
    /// Create a new saved relation from an upstream relation.
    pub(crate) fn new(upstream: R) -> Self {
        SavedRelation {
            state: Rc::new(RefCell::new(SavedState {
                upstream,
                consumer_queues: Vec::new(),
                commit_id: None,
                last_update_commit_id: CommitId::default(),
            })),
        }
    }

    /// Create a new saved relation with commit ID tracking for optimization.
    pub(crate) fn with_commit_id(upstream: R, commit_id: Rc<Cell<CommitId>>) -> Self {
        SavedRelation {
            state: Rc::new(RefCell::new(SavedState {
                upstream,
                consumer_queues: Vec::new(),
                commit_id: Some(commit_id),
                last_update_commit_id: CommitId::default(),
            })),
        }
    }

    /// Get a relation handle for this saved relation.
    ///
    /// Each call returns a new consumer. All consumers receive the same changes.
    pub fn get(&self) -> SavedGetter<T, R> {
        let queue = Rc::new(RefCell::new(Multiset::new()));
        self.state.borrow_mut().consumer_queues.push(queue.clone());
        SavedGetter {
            state: self.state.clone(),
            queue,
        }
    }
}

/// A getter for a saved relation - implements Relation.
pub struct SavedGetter<T: Tuple, R: Relation<T>> {
    state: Rc<RefCell<SavedState<T, R>>>,
    queue: Rc<RefCell<Multiset<T>>>,
}

impl<T: Tuple + 'static, R: Relation<T>> Relation<T> for SavedGetter<T, R> {
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        // First pull from upstream to all queues
        self.state.borrow_mut().update();

        // Then drain our queue
        let mut queue = self.queue.borrow_mut();
        for (t, diff) in queue.iter_with_multiplicity() {
            consumer(t.clone(), diff);
        }
        queue.clear();
    }
}

/// Create a saved relation.
pub fn save<T: Tuple + 'static, R: Relation<T>>(upstream: R) -> SavedRelation<T, R> {
    SavedRelation::new(upstream)
}
