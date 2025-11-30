//! SavedRelation - for using a relation in multiple places.

use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use crate::change::{Change, Diff};
use crate::collection::Multiset;
use crate::database::commit_id::CommitId;

use super::relation::{Op, Relation};

/// Shared state for a saved relation.
struct SavedState<T, R: Op<T>> {
    upstream: Relation<R>,
    /// Separate pending multiset for each consumer.
    consumer_queues: Vec<Rc<RefCell<Multiset<T>>>>,
    /// The commit ID when we last pulled from upstream.
    last_update_commit_id: CommitId,
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
pub struct SavedRelation<T, R: Op<T>> {
    state: Rc<RefCell<SavedState<T, R>>>,
}

impl<T: Eq + Hash, R: Op<T>> SavedRelation<T, R> {
    /// Get a relation handle for this saved relation.
    ///
    /// Each call returns a new consumer. All consumers receive the same changes.
    pub fn get(&self) -> Relation<SavedGetter<T, R>> {
        let queue = Rc::new(RefCell::new(Multiset::new()));
        let commit_id = self.state.borrow().upstream.commit_id.clone();
        self.state.borrow_mut().consumer_queues.push(queue.clone());
        Relation::new(
            SavedGetter {
                state: self.state.clone(),
                queue,
            },
            commit_id,
        )
    }
}

/// A getter for a saved relation - implements Relation.
pub struct SavedGetter<T, R: Op<T>> {
    state: Rc<RefCell<SavedState<T, R>>>,
    queue: Rc<RefCell<Multiset<T>>>,
}

impl<T: Clone + Eq + Hash, R: Op<T>> Op<T> for SavedGetter<T, R> {
    fn foreach(&mut self, consumer: &mut dyn FnMut(T, Diff)) {
        // First pull from upstream to all queues
        self.state.borrow_mut().update();

        // Then drain our queue
        let mut queue = self.queue.borrow_mut();
        for (t, diff) in queue.drain() {
            consumer(t, diff);
        }
    }
}

/// Create a saved relation.
pub fn save<T: Eq + Hash, R: Op<T>>(upstream: Relation<R>) -> SavedRelation<T, R> {
    SavedRelation {
        state: Rc::new(RefCell::new(SavedState {
            upstream,
            consumer_queues: Vec::new(),
            last_update_commit_id: CommitId::default(),
        })),
    }
}
