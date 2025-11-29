//! Type-erased wrappers for database-managed components.

use std::cell::RefCell;
use std::rc::Rc;

use crate::change::Diff;
use crate::database2::feedback::Variable;
use crate::database2::relational::input::InputState;
use crate::database2::relational::Relation;
use crate::Tuple;

/// Type-erased input handle operations.
pub(super) trait AnyInput {
    /// Commit staged changes to pending.
    fn commit(&self);
    /// Push a checkpoint - start recording changes.
    fn push_checkpoint(&self);
    /// Pop a checkpoint - queue undo changes, returns true if there were changes.
    fn pop_checkpoint(&self) -> bool;
}

/// Wrapper to make InputHandle type-erased.
pub(super) struct InputWrapper<T: Tuple> {
    pub(super) state: Rc<RefCell<InputState<T>>>,
    /// Stack of changes at each checkpoint level.
    checkpoint_stack: RefCell<Vec<Vec<(T, Diff)>>>,
}

impl<T: Tuple> InputWrapper<T> {
    pub(super) fn new(state: Rc<RefCell<InputState<T>>>) -> Self {
        InputWrapper {
            state,
            checkpoint_stack: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn push_initial_checkpoints(&self, depth: usize) {
        for _ in 0..depth {
            self.checkpoint_stack.borrow_mut().push(Vec::new());
        }
    }
}

impl<T: Tuple + 'static> AnyInput for InputWrapper<T> {
    fn commit(&self) {
        let mut state = self.state.borrow_mut();

        // Record staged changes to current checkpoint level before committing
        if let Some(level) = self.checkpoint_stack.borrow_mut().last_mut() {
            level.extend(state.staged.iter().cloned());
        }

        state.commit();
    }

    fn push_checkpoint(&self) {
        self.checkpoint_stack.borrow_mut().push(Vec::new());
    }

    fn pop_checkpoint(&self) -> bool {
        if let Some(changes) = self.checkpoint_stack.borrow_mut().pop() {
            if !changes.is_empty() {
                let mut state = self.state.borrow_mut();
                // Queue the inverse of all changes
                for (tuple, diff) in changes {
                    state.pending.push((tuple, Diff(-diff.0)));
                }
                return true;
            }
        }
        false
    }
}

/// Type-erased feedback operations.
pub(super) trait AnyFeedback {
    /// Push a checkpoint.
    fn push_checkpoint(&self);
    /// Pop the variable's checkpoint and revert outputs (emits -1 changes).
    fn pop_checkpoint(&self);
    /// Pull from input relation to update input_counts, then readd reachable tuples.
    fn pull_and_readd(&self);
    /// Run one step: pull from input relation, add to variable. Returns true if new output.
    fn step(&self, recording: bool) -> bool;
}

/// Wrapper to make feedback type-erased.
pub(super) struct FeedbackWrapper<T: Tuple, R: Relation<T>> {
    pub(super) variable: Rc<RefCell<Variable<T>>>,
    /// The input relation that feeds into this variable.
    input: RefCell<R>,
}

impl<T: Tuple, R: Relation<T>> FeedbackWrapper<T, R> {
    pub(super) fn new(variable: Rc<RefCell<Variable<T>>>, input: R) -> Self {
        FeedbackWrapper {
            variable,
            input: RefCell::new(input),
        }
    }

    pub(super) fn push_initial_checkpoints(&self, depth: usize) {
        for _ in 0..depth {
            self.variable.borrow_mut().push_checkpoint();
        }
    }
}

impl<T: Tuple + 'static, R: Relation<T> + 'static> AnyFeedback for FeedbackWrapper<T, R> {
    fn push_checkpoint(&self) {
        self.variable.borrow_mut().push_checkpoint();
    }

    fn pop_checkpoint(&self) {
        let mut var = self.variable.borrow_mut();
        // Pop and discard to_readd - we'll recalculate after pulling input
        let _ = var.pop_checkpoint();
    }

    fn pull_and_readd(&self) {
        let mut var = self.variable.borrow_mut();
        let mut input = self.input.borrow_mut();

        // Pull from input relation to update input_counts
        input.foreach(&mut |tuple: &T, diff: Diff| {
            var.update_input_count(tuple.clone(), diff);
        });

        // Now readd tuples that are still reachable
        var.readd_reachable();
    }

    fn step(&self, recording: bool) -> bool {
        let mut var = self.variable.borrow_mut();
        let mut input = self.input.borrow_mut();

        // Pull from input relation and add positive tuples to variable
        input.foreach(&mut |tuple: &T, diff: Diff| {
            if diff.0 > 0 {
                for _ in 0..diff.0 {
                    var.add_change(tuple.clone(), Diff(1));
                }
            }
        });

        // Check if any new output was produced
        let _ = recording; // TODO: use for checkpoint tracking
        var.has_changes()
    }
}
