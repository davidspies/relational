//! Input wrapper for type-erased input operations.

use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use crate::HashSet;
use contiguous_data::L2Vec;

use crate::database::relational::input::InputState;

/// Type-erased input handle operations.
pub(crate) trait AnyInput {
    /// Push a checkpoint - start recording inserts.
    fn push_checkpoint(&mut self);
    /// Record any new inserts (from pending) to the current checkpoint.
    /// Called during db.commit() to capture what was inserted.
    fn record_pending_inserts(&mut self);
    /// Remove all tuples inserted in the current checkpoint and pop.
    /// Used as step 1 of the database pop() algorithm.
    fn send_inverse_and_pop(&mut self);
}

/// Wrapper to make InputHandle type-erased (seen-set semantics).
pub(crate) struct InputWrapper<T> {
    /// Shared state with InputHandle and InputRelation.
    state: Rc<RefCell<InputState<T>>>,
    /// Tuples inserted at each checkpoint level (stratified).
    checkpoint_tuples: L2Vec<T>,
    /// All tuples currently tracked across all checkpoints (for dedup).
    seen: HashSet<T>,
}

impl<T> InputWrapper<T> {
    pub(crate) fn new(state: Rc<RefCell<InputState<T>>>) -> Self {
        InputWrapper {
            state,
            checkpoint_tuples: L2Vec::new(),
            seen: HashSet::new(),
        }
    }

    pub(crate) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.checkpoint_tuples.push_empty();
        }
    }
}

impl<T: Clone + Eq + Hash> AnyInput for InputWrapper<T> {
    fn push_checkpoint(&mut self) {
        self.checkpoint_tuples.push_empty();
    }

    fn record_pending_inserts(&mut self) {
        if self.checkpoint_tuples.is_empty() {
            // No checkpoint - just drain to discard
            self.state.borrow_mut().take_new_inserts().for_each(drop);
            return;
        }

        let mut state = self.state.borrow_mut();
        for tuple in state.take_new_inserts() {
            if self.seen.insert(tuple.clone()) {
                self.checkpoint_tuples.push(tuple);
            }
        }
    }

    fn send_inverse_and_pop(&mut self) {
        for tuple in self.checkpoint_tuples.pop().unwrap() {
            self.seen.remove(&tuple);
            self.state.borrow_mut().remove(tuple);
        }
    }
}
