//! LiteralCountsSink - tracks literal counts grouped by count value.

use std::collections::BTreeMap;

use relational::Diff;
use relational::Multiset;
use relational::database::Sink;

use super::types::Lit;

/// A sink that tracks literals grouped by their clause count.
///
/// Stores `BTreeMap<count, Multiset<Lit>>` for efficient lookup of
/// literals with a given count (useful for decision heuristics like VSIDS).
#[derive(Default, Clone)]
pub struct LiteralCountsSink {
    data: BTreeMap<i64, Multiset<Lit>>,
}

impl LiteralCountsSink {
    /// Get the highest count and its literals.
    pub fn max_count(&self) -> Option<(i64, &Multiset<Lit>)> {
        self.data.last_key_value().map(|(&k, v)| (k, v))
    }
}

impl Sink<(Lit, i64)> for LiteralCountsSink {
    fn apply(&mut self, (lit, count): (Lit, i64), diff: Diff) {
        let ms = self.data.entry(count).or_default();
        ms.update(lit, diff);
        if ms.is_empty() {
            self.data.remove(&count);
        }
    }
}
