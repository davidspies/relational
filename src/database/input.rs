//! Input relations and mutation operations for Database.

use crate::Tuple;
use crate::change::Change;
use crate::collection::Multiset;
use crate::relation::Relation;

use super::Database;

impl Database {
    /// Create a new input relation.
    pub fn create_input<T: Tuple + Send + Sync>(&mut self, name: &str) -> Relation<T> {
        let id = self.graph.create_input::<T>(name);
        Relation::new(id)
    }

    /// Create a new persistent input relation.
    /// Changes to this relation survive pop().
    pub fn create_persistent_input<T: Tuple + Send + Sync>(&mut self, name: &str) -> Relation<T> {
        let id = self.graph.create_persistent_input::<T>(name);
        Relation::new(id)
    }

    /// Insert a tuple into a relation.
    pub fn insert<T: Tuple + Send + Sync>(&mut self, rel: Relation<T>, tuple: T) {
        if self.checkpoint_stack.is_recording() && !self.graph.get(rel.id).is_persistent() {
            self.checkpoint_stack
                .record(rel.id, vec![Change::insert(tuple.clone())]);
        }

        let node = self.graph.get_mut(rel.id);

        if let Some(changes) = node
            .pending_changes
            .as_any_mut()
            .downcast_mut::<Vec<Change<T>>>()
        {
            changes.push(Change::insert(tuple.clone()));
        }

        if let Some(coll) = node.state.as_any_mut().downcast_mut::<Multiset<T>>() {
            coll.insert(tuple);
        }

        self.graph.mark_dirty(rel.id);
    }

    /// Delete a tuple from a relation.
    pub fn delete<T: Tuple + Send + Sync>(&mut self, rel: Relation<T>, tuple: T) {
        if self.checkpoint_stack.is_recording() && !self.graph.get(rel.id).is_persistent() {
            self.checkpoint_stack
                .record(rel.id, vec![Change::delete(tuple.clone())]);
        }

        let node = self.graph.get_mut(rel.id);

        if let Some(changes) = node
            .pending_changes
            .as_any_mut()
            .downcast_mut::<Vec<Change<T>>>()
        {
            changes.push(Change::delete(tuple.clone()));
        }

        if let Some(coll) = node.state.as_any_mut().downcast_mut::<Multiset<T>>() {
            coll.delete(tuple);
        }

        self.graph.mark_dirty(rel.id);
    }

    /// Commit staged changes and propagate through the dataflow graph.
    pub fn commit(&mut self) {
        if self.stratified_ops.is_empty() {
            self.recompute_all();
            self.graph.clear_dirty();
        } else {
            self.run_stratified_fixpoint();
        }
    }

    /// Check if the last fixpoint computation was interrupted.
    pub fn was_interrupted(&self) -> bool {
        self.interrupted
    }
}
