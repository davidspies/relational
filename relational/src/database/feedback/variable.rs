//! Variable for tracking iterative computation state.

use std::hash::Hash;

use contiguous_data::{Diff, Multiset};
use derive_where::derive_where;

/// A variable in an iterative computation.
///
/// This is a simple output buffer. The wrapper (FeedbackWrapper or
/// FeedbackWithIdWrapper) handles the seen-set, checkpoint tracking,
/// and reachability logic.
#[derive_where(Default)]
pub struct Variable<T> {
    /// Staged changes (not yet committed).
    staged: Multiset<T>,
    /// Pending changes (committed, ready to be pulled).
    pending: Multiset<T>,
}

impl<T: Clone + Eq + Hash> Variable<T> {
    /// Create a new empty variable.
    pub fn new() -> Self {
        Self::default()
    }

    /// Emit a +1 for this tuple. Caller is responsible for seen-set checks.
    pub(crate) fn emit(&mut self, tuple: T) {
        self.staged.update(tuple, 1);
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
}
