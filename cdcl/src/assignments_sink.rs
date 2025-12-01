//! AssignmentsSink - tracks (Lit, Level) with lookup by Lit.

use std::collections::HashMap;

use relational::Multiset;
use relational::database::Sink;

use super::types::{Level, Lit};

/// A sink that tracks assignments with efficient lookup by literal.
#[derive(Default, Clone)]
pub struct AssignmentsSink {
    data: HashMap<Lit, Multiset<Level>>,
}

impl AssignmentsSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the level at which a literal was assigned (if any with positive multiplicity).
    pub fn get(&self, lit: &Lit) -> Option<Level> {
        self.data.get(lit).and_then(|ms| ms.iter().next().copied())
    }

    /// Iterate over all assignments with positive multiplicity.
    pub fn iter(&self) -> impl Iterator<Item = (Lit, Level)> + '_ {
        self.data
            .iter()
            .flat_map(|(&lit, ms)| ms.iter().map(move |&level| (lit, level)))
    }
}

impl Sink<(Lit, Level)> for AssignmentsSink {
    fn dump_all(&mut self, incoming: &mut Multiset<(Lit, Level)>) {
        for ((lit, level), diff) in incoming.drain() {
            let ms = self.data.entry(lit).or_default();
            ms.update(level, diff);
            if ms.is_empty() {
                self.data.remove(&lit);
            }
        }
    }
}
