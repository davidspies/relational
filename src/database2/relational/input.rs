//! InputHandle and InputRelation for input relations.
//!
//! Inputs must be created through Database2::create_input(), which manages
//! commit and checkpoint operations centrally.

use std::cell::RefCell;
use std::rc::Rc;

use crate::change::Diff;
use crate::Tuple;

use super::relation::Relation;

/// The internal state of an input relation.
pub(crate) struct InputState<T: Tuple> {
    /// Changes waiting to be committed.
    pub staged: Vec<(T, Diff)>,
    /// Changes that have been committed and are ready to be pulled.
    pub pending: Vec<(T, Diff)>,
}

impl<T: Tuple> InputState<T> {
    pub(crate) fn new() -> Self {
        InputState {
            staged: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// Commit staged changes - moves them to pending.
    pub(crate) fn commit(&mut self) {
        self.pending.append(&mut self.staged);
    }
}

/// A relation backed by an InputHandle.
pub struct InputRelation<T: Tuple> {
    pub(crate) state: Rc<RefCell<InputState<T>>>,
}

impl<T: Tuple + 'static> Relation<T> for InputRelation<T> {
    fn foreach(&mut self, f: &mut dyn FnMut(&T, Diff)) {
        let mut state = self.state.borrow_mut();
        for (t, diff) in state.pending.drain(..) {
            f(&t, diff);
        }
    }
}

impl<T: Tuple> Clone for InputRelation<T> {
    fn clone(&self) -> Self {
        InputRelation {
            state: self.state.clone(),
        }
    }
}

/// A handle for inserting and deleting tuples from an input relation.
///
/// Use `insert` and `delete` to stage changes. Changes become visible
/// after calling `db.commit()` on the Database2 that created this input.
pub struct InputHandle<T: Tuple> {
    pub(crate) state: Rc<RefCell<InputState<T>>>,
}

impl<T: Tuple + 'static> InputHandle<T> {
    /// Insert a tuple into the input relation (staged until db.commit()).
    pub fn insert(&mut self, tuple: T) {
        self.state.borrow_mut().staged.push((tuple, Diff(1)));
    }

    /// Delete a tuple from the input relation (staged until db.commit()).
    pub fn delete(&mut self, tuple: T) {
        self.state.borrow_mut().staged.push((tuple, Diff(-1)));
    }

    /// Get a relation handle for this input.
    pub fn relation(&self) -> InputRelation<T> {
        InputRelation {
            state: self.state.clone(),
        }
    }
}
