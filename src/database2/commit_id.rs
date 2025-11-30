//! Commit ID tracking for database mutations.

/// A monotonically increasing commit ID that tracks database mutations.
///
/// This counter is incremented each feedback iteration.
/// The counter never decreases, even during backtracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct CommitId(pub(crate) u64);

impl CommitId {
    /// Create a new CommitId from a raw value.
    pub fn new(id: u64) -> Self {
        CommitId(id)
    }

    /// Get the raw u64 value.
    pub fn raw(self) -> u64 {
        self.0
    }
}
