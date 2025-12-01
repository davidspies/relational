//! Working set for conflict analysis, tracking literals split by decision level.

use std::collections::{BTreeMap, HashSet};

use relational::database::CommitId;

use crate::assignments_sink::AssignmentsSink;
use crate::cause_sink::CauseSink;
use crate::types::{Level, Lit};

/// Working set for conflict analysis, split by level for efficient access.
pub(super) struct WorkingSet {
    /// Literals at the current decision level, ordered by commit ID (most recent last).
    /// Multiple literals can share the same commit ID, so we use a set per commit.
    at_current_level: BTreeMap<CommitId, HashSet<Lit>>,
    /// Literals at other levels (no ordering needed).
    at_other_levels: HashSet<Lit>,
}

impl WorkingSet {
    pub(super) fn new() -> Self {
        Self {
            at_current_level: BTreeMap::new(),
            at_other_levels: HashSet::new(),
        }
    }

    /// Insert a literal into the appropriate collection based on its level.
    pub(super) fn insert(
        &mut self,
        lit: Lit,
        current_level: Level,
        assignments: &AssignmentsSink,
        causes: &CauseSink,
    ) {
        let at_current = assignments.get(&lit) == Some(current_level);
        let in_current = self
            .at_current_level
            .values()
            .any(|lits| lits.contains(&lit));
        let in_other = self.at_other_levels.contains(&lit);

        // Same literal should never appear at multiple levels
        if at_current {
            assert!(!in_other, "Literal {:?} appears at multiple levels", lit);
        } else {
            assert!(!in_current, "Literal {:?} appears at multiple levels", lit);
        }

        // Skip if already in the correct collection
        if in_current || in_other {
            return;
        }

        if at_current {
            if let Some(commit_id) = causes.get_commit_id(lit) {
                self.at_current_level
                    .entry(commit_id)
                    .or_default()
                    .insert(lit);
            } else {
                // Decision literal at current level - treat as other level for simplicity
                self.at_other_levels.insert(lit);
            }
        } else {
            self.at_other_levels.insert(lit);
        }
    }

    /// Get the most recently assigned literal at current level (highest commit ID).
    /// Returns one literal from the most recent commit.
    pub(super) fn pop_most_recent(&mut self) -> Option<(CommitId, Lit)> {
        let (&commit_id, lits) = self.at_current_level.last_key_value()?;
        let lit = *lits.iter().next()?;
        // Remove this literal from the set
        let lits = self.at_current_level.get_mut(&commit_id).unwrap();
        lits.remove(&lit);
        if lits.is_empty() {
            self.at_current_level.remove(&commit_id);
        }
        Some((commit_id, lit))
    }

    /// Count of literals at current level.
    pub(super) fn count_at_current(&self) -> usize {
        self.at_current_level.values().map(|s| s.len()).sum()
    }

    /// Iterate over all literals in the working set.
    pub(super) fn iter(&self) -> impl Iterator<Item = Lit> + '_ {
        self.at_current_level
            .values()
            .flat_map(|s| s.iter())
            .copied()
            .chain(self.at_other_levels.iter().copied())
    }
}
