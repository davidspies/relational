//! CauseSink - accumulates causes into a nested HashMap structure.

use std::collections::{BTreeMap, HashMap};

use relational::Diff;
use relational::Multiset;
use relational::database::{CommitId, Sink};

use super::types::{ClauseId, Level, Lit};

/// A single cause entry: which clause at which level caused a propagation.
type CauseEntry = (ClauseId, Level);

/// Causes grouped by commit ID.
type CommitCauses = BTreeMap<CommitId, Multiset<CauseEntry>>;

/// The full cause data: for each literal, its causes by commit.
type CauseData = HashMap<Lit, CommitCauses>;

/// A sink that accumulates cause information for conflict analysis.
///
/// For each literal, tracks when it was derived (by CommitId) and what
/// clause/level caused each derivation.
#[derive(Default, Clone)]
pub struct CauseSink {
    data: CauseData,
}

impl CauseSink {
    /// Get the reason clause for a literal (the clause from the earliest commit).
    pub fn get_reason(&self, lit: Lit) -> Option<ClauseId> {
        self.data.get(&lit).and_then(|commits| {
            commits.values().next().and_then(|multiset| {
                multiset
                    .iter()
                    .find(|(cid, _)| !cid.is_decision())
                    .map(|(cid, _)| *cid)
            })
        })
    }

    /// Get the earliest commit ID for a literal.
    pub fn get_commit_id(&self, lit: Lit) -> Option<CommitId> {
        self.data
            .get(&lit)
            .and_then(|commits| commits.keys().next().copied())
    }
}

impl Sink<((Lit, CommitId), (ClauseId, Level))> for CauseSink {
    fn apply(&mut self, tuple: ((Lit, CommitId), (ClauseId, Level)), diff: Diff) {
        let ((lit, commit_id), (clause_id, level)) = tuple;
        let commits = self.data.entry(lit).or_default();
        let multiset = commits.entry(commit_id).or_default();
        multiset.update((clause_id, level), diff);
        if multiset.is_empty() {
            commits.remove(&commit_id);
        }
    }
}
