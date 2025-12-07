//! Interrupt wrapper for type-erased interrupt operations.

use std::hash::Hash;

use contiguous_data::Multiset;

use crate::database::commit_id::CommitId;
use crate::database::{Relation, relational::Op};

/// Type-erased interrupt operations.
pub(crate) trait AnyInterrupt {
    /// Check if the interrupt condition is met (relation has non-zero entries).
    fn check(&mut self) -> bool;
}

/// Wrapper for interrupt - checks if a relation has any non-zero entries.
pub(crate) struct InterruptWrapper<T, R: Op<T>> {
    input: Relation<R>,
    /// Tracks the actual state of the relation.
    state: Multiset<T>,
    /// The commit ID when we last pulled from input.
    last_update_commit_id: CommitId,
}

impl<T: Clone + Eq + Hash, R: Op<T>> InterruptWrapper<T, R> {
    pub(crate) fn new(input: Relation<R>) -> Self {
        InterruptWrapper {
            input,
            state: Multiset::new(),
            last_update_commit_id: CommitId::default(),
        }
    }
}

impl<T: Clone + Eq + Hash, R: Op<T>> AnyInterrupt for InterruptWrapper<T, R> {
    fn check(&mut self) -> bool {
        let current = self.input.commit_id.get();
        assert!(current != self.last_update_commit_id);
        self.last_update_commit_id = current;

        self.input.dump_to_multiset(&mut self.state);
        !self.state.is_empty()
    }
}
