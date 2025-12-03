//! Sink trait for receiving relation changes.

use std::hash::Hash;

use contiguous_data::{L2Multiset, Multiset};

/// A sink that can receive changes from a relation.
///
/// Implement this trait to create custom data structures that accumulate
/// relation changes in different ways.
pub trait Sink<T> {
    /// Apply a single change (tuple with diff) to the sink.
    fn dump_all(&mut self, incoming: &mut Multiset<T>);
}

impl<T: Eq + Hash> Sink<T> for Multiset<T> {
    fn dump_all(&mut self, incoming: &mut Multiset<T>) {
        for (t, diff) in incoming.drain() {
            self.update(t, diff);
        }
    }
}

impl<K: Eq + Hash + Clone, V: Eq + Hash + Clone> Sink<(K, V)> for L2Multiset<K, V> {
    fn dump_all(&mut self, incoming: &mut Multiset<(K, V)>) {
        for ((k, v), diff) in incoming.drain() {
            self.update(k, v, diff);
        }
    }
}
