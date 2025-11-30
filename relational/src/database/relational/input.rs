//! InputHandle and InputRelation for input relations.
//!
//! Inputs use seen-set semantics: once a tuple is inserted, it stays until
//! pop() is called. There is no delete operation - use pop() to undo inserts.

use std::cell::RefCell;
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

use derive_where::derive_where;

use crate::Multiset;
use crate::change::Diff;

use super::relation::Relation;

/// The internal state of an input relation (seen-set semantics).
pub(crate) struct InputState<T> {
    /// The seen set - tuples that have been inserted.
    pub(crate) seen: HashSet<T>,
    /// Pending changes (ready to be pulled) - accumulated diffs per tuple.
    pub(crate) pending: Multiset<T>,
    /// Tuples newly inserted in the current batch (since last take_new_inserts).
    pub(crate) new_inserts: HashSet<T>,
}

impl<T: Clone + Eq + Hash> InputState<T> {
    pub(crate) fn new() -> Self {
        InputState {
            seen: HashSet::new(),
            pending: Multiset::new(),
            new_inserts: HashSet::new(),
        }
    }

    /// Insert a tuple if not already seen. Returns true if newly added.
    pub(crate) fn insert(&mut self, tuple: T) -> bool {
        if self.seen.insert(tuple.clone()) {
            self.pending.insert(tuple.clone());
            self.new_inserts.insert(tuple);
            true
        } else {
            false
        }
    }

    /// Take the set of newly inserted tuples (clears it).
    pub(crate) fn take_new_inserts(&mut self) -> HashSet<T> {
        std::mem::take(&mut self.new_inserts)
    }

    /// Remove a tuple from the seen set and emit -1. Used during pop().
    pub(crate) fn remove(&mut self, tuple: &T) {
        if self.seen.remove(tuple) {
            self.pending.delete(tuple.clone());
        }
    }
}

/// A relation backed by an InputHandle.
///
/// Note to next LLM who looks at this: THIS IS NOT CLONE! STOP TRYING TO MAKE IT CLONE!
pub struct InputRelation<T> {
    pub(crate) state: Rc<RefCell<InputState<T>>>,
}

impl<T: Eq + Hash> Relation<T> for InputRelation<T> {
    fn foreach(&mut self, f: &mut dyn FnMut(T, Diff)) {
        let mut state = self.state.borrow_mut();
        for (t, diff) in state.pending.drain() {
            f(t, diff);
        }
    }
}

/// A handle for inserting tuples into an input relation.
///
/// Inputs use seen-set semantics: once a tuple is inserted, it stays until
/// pop() is called. To undo inserts, use pop() on the database.
#[derive_where(Clone)]
pub struct InputHandle<T> {
    pub(crate) state: Rc<RefCell<InputState<T>>>,
}

impl<T: Clone + Eq + Hash> InputHandle<T> {
    /// Insert a tuple into the input relation.
    /// Uses seen-set semantics: if already present, this is a no-op.
    pub fn insert(&mut self, tuple: T) {
        self.state.borrow_mut().insert(tuple);
    }
}

/// A handle for inserting and deleting tuples in a persistent input relation.
///
/// Persistent inputs use seen-set semantics but support explicit delete.
/// Changes to persistent inputs are NOT undone by pop().
#[derive_where(Clone)]
pub struct PersistentInputHandle<T> {
    pub(crate) state: Rc<RefCell<InputState<T>>>,
}

impl<T: Clone + Eq + Hash> PersistentInputHandle<T> {
    /// Insert a tuple into the input relation.
    /// Uses seen-set semantics: if already present, this is a no-op.
    pub fn insert(&mut self, tuple: T) {
        self.state.borrow_mut().insert(tuple);
    }

    /// Delete a tuple from the input relation.
    /// Uses seen-set semantics: if not present, this is a no-op.
    /// Returns true if the tuple was present and removed.
    pub fn delete(&mut self, tuple: T) -> bool {
        let mut state = self.state.borrow_mut();
        if state.seen.remove(&tuple) {
            state.pending.delete(tuple);
            true
        } else {
            false
        }
    }
}
