//! Input wrapper for type-erased input operations.

use std::cell::RefCell;
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

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
    /// Stack of tuples inserted at each checkpoint level.
    checkpoint_stack: Vec<HashSet<T>>,
}

impl<T> InputWrapper<T> {
    pub(crate) fn new(state: Rc<RefCell<InputState<T>>>) -> Self {
        InputWrapper {
            state,
            checkpoint_stack: Vec::new(),
        }
    }

    pub(crate) fn push_initial_checkpoints(&mut self, depth: usize) {
        for _ in 0..depth {
            self.checkpoint_stack.push(HashSet::new());
        }
    }
}

impl<T: Clone + Eq + Hash> AnyInput for InputWrapper<T> {
    fn push_checkpoint(&mut self) {
        self.checkpoint_stack.push(HashSet::new());
    }

    fn record_pending_inserts(&mut self) {
        if let Some(level) = self.checkpoint_stack.last_mut() {
            let new_inserts = self.state.borrow_mut().take_new_inserts();
            level.extend(new_inserts);
        } else {
            self.state.borrow_mut().take_new_inserts();
        }
    }

    fn send_inverse_and_pop(&mut self) {
        if let Some(tuples) = self.checkpoint_stack.pop() {
            let mut state = self.state.borrow_mut();
            for tuple in tuples {
                state.remove(&tuple);
            }
        }
    }
}
