//! Restart strategies for CDCL solver.
//!
//! Implements the Luby sequence restart strategy, which provides a good
//! balance between exploration and exploitation.

/// Generates the Luby sequence: 1, 1, 2, 1, 1, 2, 4, 1, 1, 2, 1, 1, 2, 4, 8, ...
///
/// The sequence is defined recursively:
/// - L(1) = 1
/// - L(2^k) = 2^(k-1) for k >= 1
/// - L(2^k + i) = L(i) for 1 <= i < 2^k
fn luby(mut i: u64) -> u64 {
    // Find the highest power of 2 that is <= i
    // The sequence has a fractal structure based on powers of 2
    loop {
        // Find k such that 2^k - 1 < i <= 2^(k+1) - 1
        let mut k = 0u64;
        while (1u64 << (k + 1)) - 1 < i {
            k += 1;
        }

        // Now 2^k - 1 < i <= 2^(k+1) - 1
        // If i == 2^(k+1) - 1, return 2^k
        if i == (1u64 << (k + 1)) - 1 {
            return 1u64 << k;
        }

        // Otherwise, subtract 2^k - 1 and recurse
        // (indices 1..2^k-1 map to themselves in the subsequence)
        i -= (1u64 << k) - 1;

        // For safety, handle the edge case
        if i == 0 {
            return 1;
        }
    }
}

/// Restart strategy state.
pub(crate) struct RestartStrategy {
    /// Base number of conflicts between restarts.
    base_interval: u64,
    /// Current position in the Luby sequence.
    luby_index: u64,
    /// Conflicts since last restart.
    conflicts_since_restart: u64,
    /// Total number of restarts performed.
    pub(crate) restarts: u64,
}

impl RestartStrategy {
    /// Create a new restart strategy with the given base interval.
    ///
    /// The actual restart interval is `base_interval * luby(i)` where `i`
    /// is the current position in the Luby sequence.
    pub(crate) fn new(base_interval: u64) -> Self {
        Self {
            base_interval,
            luby_index: 1,
            conflicts_since_restart: 0,
            restarts: 0,
        }
    }

    /// Record a conflict. Returns true if a restart should be triggered.
    pub(crate) fn on_conflict(&mut self) -> bool {
        self.conflicts_since_restart += 1;
        let limit = self.base_interval * luby(self.luby_index);
        self.conflicts_since_restart >= limit
    }

    /// Acknowledge that a restart was performed.
    pub(crate) fn on_restart(&mut self) {
        self.conflicts_since_restart = 0;
        self.luby_index += 1;
        self.restarts += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_luby_sequence() {
        // First 15 values of the Luby sequence
        let expected = [1, 1, 2, 1, 1, 2, 4, 1, 1, 2, 1, 1, 2, 4, 8];
        for (i, &expected_val) in expected.iter().enumerate() {
            assert_eq!(luby((i + 1) as u64), expected_val, "luby({}) failed", i + 1);
        }
    }

    #[test]
    fn test_restart_strategy() {
        let mut strategy = RestartStrategy::new(100);

        // First interval should be 100 * luby(1) = 100 * 1 = 100
        for _ in 0..99 {
            assert!(!strategy.on_conflict());
        }
        assert!(strategy.on_conflict());
        strategy.on_restart();

        // Second interval should be 100 * luby(2) = 100 * 1 = 100
        for _ in 0..99 {
            assert!(!strategy.on_conflict());
        }
        assert!(strategy.on_conflict());
        strategy.on_restart();

        // Third interval should be 100 * luby(3) = 100 * 2 = 200
        for _ in 0..199 {
            assert!(!strategy.on_conflict());
        }
        assert!(strategy.on_conflict());
    }
}
