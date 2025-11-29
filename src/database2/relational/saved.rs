//! SavedRelation - for using a relation in multiple places.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::change::Diff;
use crate::Tuple;

use super::relation::Relation;

/// Internal state for a saved relation.
struct SavedState<T: Tuple> {
    /// Pending changes to deliver to consumers
    pending: Vec<(T, Diff)>,
    /// How many consumers exist
    num_consumers: usize,
    /// Which consumers have read the current batch
    consumed: HashSet<usize>,
}

impl<T: Tuple> SavedState<T> {
    fn new() -> Self {
        SavedState {
            pending: Vec::new(),
            num_consumers: 0,
            consumed: HashSet::new(),
        }
    }
}

/// A saved relation that can be used in multiple places.
///
/// Call `.get()` to obtain a relation that can be used in the dataflow graph.
/// Each call to `.get()` returns a new consumer of the saved data.
pub struct SavedRelation<T: Tuple, R: Relation<T>> {
    upstream: R,
    state: Rc<RefCell<SavedState<T>>>,
}

impl<T: Tuple + 'static, R: Relation<T>> SavedRelation<T, R> {
    /// Create a new saved relation from an upstream relation.
    pub fn new(upstream: R) -> Self {
        SavedRelation {
            upstream,
            state: Rc::new(RefCell::new(SavedState::new())),
        }
    }

    /// Get a relation handle for this saved relation.
    ///
    /// Each call returns a new consumer. All consumers receive the same changes.
    pub fn get(&mut self) -> SavedGetter<T> {
        let mut state = self.state.borrow_mut();
        let id = state.num_consumers;
        state.num_consumers += 1;
        drop(state);

        SavedGetter {
            state: self.state.clone(),
            id,
        }
    }

    /// Pull changes from upstream and make them available to all getters.
    /// This should be called once per "tick" before any getters are used.
    pub fn update(&mut self) {
        let state = self.state.clone();
        self.upstream.foreach(&mut |t, diff| {
            state.borrow_mut().pending.push((t.clone(), diff));
        });
    }
}

/// A getter for a saved relation - implements Relation.
pub struct SavedGetter<T: Tuple> {
    state: Rc<RefCell<SavedState<T>>>,
    id: usize,
}

impl<T: Tuple + 'static> Relation<T> for SavedGetter<T> {
    fn foreach(&mut self, consumer: &mut dyn FnMut(&T, Diff)) {
        let mut state = self.state.borrow_mut();

        // Only deliver if this getter hasn't consumed yet
        if !state.consumed.contains(&self.id) {
            for (t, diff) in &state.pending {
                consumer(t, *diff);
            }
            state.consumed.insert(self.id);

            // Clear when all consumers have read
            if state.consumed.len() == state.num_consumers {
                state.pending.clear();
                state.consumed.clear();
            }
        }
    }
}

/// Create a saved relation.
pub fn save<T: Tuple + 'static, R: Relation<T>>(upstream: R) -> SavedRelation<T, R> {
    SavedRelation::new(upstream)
}
