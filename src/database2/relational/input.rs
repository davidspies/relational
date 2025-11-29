//! InputHandle and InputRelation for input relations.

use std::cell::RefCell;
use std::rc::Rc;

use crate::change::Diff;
use crate::Tuple;

use super::relation::Relation;

/// The internal state of an input relation.
struct InputState<T: Tuple> {
    pending: Vec<(T, Diff)>,
}

impl<T: Tuple> InputState<T> {
    fn new() -> Self {
        InputState {
            pending: Vec::new(),
        }
    }
}

/// A relation backed by an InputHandle.
pub struct InputRelation<T: Tuple> {
    state: Rc<RefCell<InputState<T>>>,
}

impl<T: Tuple + 'static> Relation<T> for InputRelation<T> {
    fn foreach(&mut self, f: &mut dyn FnMut(&T, Diff)) {
        let mut state = self.state.borrow_mut();
        for (t, diff) in state.pending.drain(..) {
            f(&t, diff);
        }
    }
}

/// A handle for inserting and deleting tuples from an input relation.
pub struct InputHandle<T: Tuple> {
    state: Rc<RefCell<InputState<T>>>,
}

impl<T: Tuple + 'static> InputHandle<T> {
    /// Insert a tuple into the input relation.
    pub fn insert(&mut self, tuple: T) {
        self.state.borrow_mut().pending.push((tuple, Diff(1)));
    }

    /// Delete a tuple from the input relation.
    pub fn delete(&mut self, tuple: T) {
        self.state.borrow_mut().pending.push((tuple, Diff(-1)));
    }

    /// Get a relation handle for this input.
    pub fn relation(&self) -> InputRelation<T> {
        InputRelation {
            state: self.state.clone(),
        }
    }
}

/// Create an input relation and return both handles.
pub fn create_input<T: Tuple + 'static>() -> (InputHandle<T>, InputRelation<T>) {
    let state = Rc::new(RefCell::new(InputState::new()));
    let handle = InputHandle {
        state: state.clone(),
    };
    let relation = InputRelation { state };
    (handle, relation)
}
