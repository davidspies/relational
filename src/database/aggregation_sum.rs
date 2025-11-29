//! Sum and count aggregation operators for Database.

use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::Relation;
use crate::Tuple;

use super::Database;

impl Database {
    /// Group by key and compute sum of values.
    ///
    /// For each distinct key K, outputs (K, sum(V)) where V are all i64 values
    /// associated with that key, weighted by multiplicity.
    ///
    /// Uses a HashMap counter internally for O(1) updates.
    pub fn group_sum<T, K, FK, FV>(
        &mut self,
        input: Relation<T>,
        key_fn: FK,
        value_fn: FV,
    ) -> Relation<(K, i64)>
    where
        T: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        FK: Fn(&T) -> K + Send + Sync + Clone + 'static,
        FV: Fn(&T) -> i64 + Send + Sync + Clone + 'static,
    {
        let input_id = input.id;
        let key_fn_clone = key_fn.clone();
        let value_fn_clone = value_fn.clone();

        let id = self.graph.create_derived::<(K, i64)>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<(K, i64)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(K, i64)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function using GroupSumState
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .expect("type mismatch in group_sum recompute")
                .clone();
            let (_, output) =
                operators::group_sum_init(&input_coll, &key_fn_clone, &value_fn_clone);
            Box::new(output) as Box<dyn AnyCollection>
        }));

        // Compute initial state using GroupSumState
        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .expect("type mismatch in group_sum initial")
            .clone();
        let (_, output) = operators::group_sum_init(&input_coll, &key_fn, &value_fn);
        self.graph.get_mut(id).state = Box::new(output);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(K, i64)>());

        Relation::new(id)
    }

    /// Group by key and count tuples.
    ///
    /// For each distinct key K, outputs (K, count) where count is the number
    /// of tuples with that key (weighted by multiplicity).
    ///
    /// Implemented as group_sum with value_fn = |_| 1.
    pub fn group_count<T, K, FK>(&mut self, input: Relation<T>, key_fn: FK) -> Relation<(K, i64)>
    where
        T: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        FK: Fn(&T) -> K + Send + Sync + Clone + 'static,
    {
        self.group_sum(input, key_fn, |_| 1)
    }

    /// Compute the maximum value in a relation (convenience method).
    ///
    /// Returns a relation containing at most one tuple: the maximum value.
    pub fn max<T>(&mut self, input: Relation<T>) -> Relation<T>
    where
        T: Tuple + Ord + Send + Sync,
    {
        // Use group_max with a constant key, then project out just the value
        let with_key = self.group_max(input, |_| (), |t| t.clone());
        self.map(with_key, |(_, v)| v.clone())
    }

    /// Compute the minimum value in a relation (convenience method).
    ///
    /// Returns a relation containing at most one tuple: the minimum value.
    pub fn min<T>(&mut self, input: Relation<T>) -> Relation<T>
    where
        T: Tuple + Ord + Send + Sync,
    {
        // Use group_min with a constant key, then project out just the value
        let with_key = self.group_min(input, |_| (), |t| t.clone());
        self.map(with_key, |(_, v)| v.clone())
    }
}
