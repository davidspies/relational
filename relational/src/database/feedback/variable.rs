//! Variable for tracking iterative computation state.

use std::hash::Hash;

use contiguous_data::{Diff, L2Vec, Multiset};
use derive_where::derive_where;

/// A variable in an iterative computation.
///
/// This is a simple output buffer with checkpoint support. The wrapper
/// (FeedbackWrapper or FeedbackWithIdWrapper) handles the seen-set and
/// reachability logic.
#[derive_where(Default)]
pub struct Variable<T> {
    /// Staged changes (not yet committed).
    staged: Multiset<T>,
    /// Pending changes (committed, ready to be pulled).
    pending: Multiset<T>,
    /// Stack of outputs added at each checkpoint level.
    outputs_by_checkpoint: L2Vec<T>,
}

impl<T: Clone + Eq + Hash> Variable<T> {
    /// Create a new empty variable.
    pub fn new() -> Self {
        Self::default()
    }

    /// Emit a +1 for this tuple. Caller is responsible for seen-set checks.
    pub(crate) fn emit(&mut self, tuple: T) {
        self.staged.update(tuple.clone(), 1);
        if !self.outputs_by_checkpoint.is_empty() {
            self.outputs_by_checkpoint.push(tuple);
        }
    }

    /// Emit a -1 for this tuple (used during pop inverse).
    pub(crate) fn emit_inverse(&mut self, tuple: &T) {
        self.staged.update(tuple.clone(), -1);
    }

    /// Take the pending changes (empties the buffer).
    pub(crate) fn drain_pending(&mut self) -> impl Iterator<Item = (T, Diff)> {
        self.pending.drain()
    }

    /// Commit staged changes to pending.
    pub(crate) fn commit(&mut self) {
        for (tuple, diff) in self.staged.drain() {
            self.pending.update(tuple, diff);
        }
    }

    /// Push a new checkpoint level.
    pub(crate) fn push_checkpoint(&mut self) {
        self.outputs_by_checkpoint.push_empty();
    }

    /// Get an iterator over the last checkpoint's contents (without popping).
    pub(crate) fn last_checkpoint(&self) -> impl Iterator<Item = &T> {
        self.outputs_by_checkpoint.last().into_iter().flatten()
    }

    /// Pop the checkpoint and return an iterator over its contents.
    pub(crate) fn pop_checkpoint_drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.outputs_by_checkpoint.pop().into_iter().flatten()
    }
}
