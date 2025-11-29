//! Query operations for Database.

use crate::Tuple;
use crate::change::Diff;
use crate::collection::Multiset;
use crate::relation::Relation;

use super::Database;

impl Database {
    /// Iterate over tuples in a relation.
    pub fn iter<T: Tuple + Send + Sync>(&self, rel: Relation<T>) -> impl Iterator<Item = &T> {
        self.graph
            .get(rel.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .into_iter()
            .flat_map(|c| c.iter())
    }

    /// Collect relation contents into a Vec.
    pub fn collect<T: Tuple + Send + Sync>(&self, rel: Relation<T>) -> Vec<T> {
        self.iter(rel).cloned().collect()
    }

    /// Iterate over tuples with their multiplicities.
    pub fn iter_with_multiplicity<T: Tuple + Send + Sync>(
        &self,
        rel: Relation<T>,
    ) -> impl Iterator<Item = (&T, Diff)> {
        self.graph
            .get(rel.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .into_iter()
            .flat_map(|c| c.iter_with_multiplicity())
    }

    /// Get the multiplicity of a specific tuple in a relation.
    pub fn multiplicity<T: Tuple + Send + Sync>(&self, rel: Relation<T>, tuple: &T) -> Diff {
        self.graph
            .get(rel.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .map(|c| c.get(tuple))
            .unwrap_or(Diff(0))
    }
}
