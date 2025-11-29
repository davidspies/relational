//! Aggregation operators for Database.

use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::Relation;
use crate::Tuple;

use super::Database;

impl Database {
    /// Group by key and compute maximum value.
    ///
    /// For each distinct key K, outputs (K, max(V)) where V are all values
    /// associated with that key.
    ///
    /// Uses BTreeMap internally for O(1) max lookup per group.
    pub fn group_max<T, K, V, FK, FV>(
        &mut self,
        input: Relation<T>,
        key_fn: FK,
        value_fn: FV,
    ) -> Relation<(K, V)>
    where
        T: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        V: Tuple + Ord + Send + Sync,
        FK: Fn(&T) -> K + Send + Sync + Clone + 'static,
        FV: Fn(&T) -> V + Send + Sync + Clone + 'static,
    {
        let input_id = input.id;
        let key_fn_clone = key_fn.clone();
        let value_fn_clone = value_fn.clone();

        let id = self.graph.create_derived::<(K, V)>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<(K, V)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(K, V)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function using BTreeMap-based group_max
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            let (_, output) =
                operators::group_max_init(&input_coll, &key_fn_clone, &value_fn_clone);
            Box::new(output) as Box<dyn AnyCollection>
        }));

        // Compute initial state using BTreeMap-based group_max
        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let (_, output) = operators::group_max_init(&input_coll, &key_fn, &value_fn);
        self.graph.get_mut(id).state = Box::new(output);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(K, V)>());

        Relation::new(id)
    }

    /// Group by key and compute minimum value.
    ///
    /// For each distinct key K, outputs (K, min(V)) where V are all values
    /// associated with that key.
    ///
    /// Uses BTreeMap internally for O(1) min lookup per group.
    pub fn group_min<T, K, V, FK, FV>(
        &mut self,
        input: Relation<T>,
        key_fn: FK,
        value_fn: FV,
    ) -> Relation<(K, V)>
    where
        T: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        V: Tuple + Ord + Send + Sync,
        FK: Fn(&T) -> K + Send + Sync + Clone + 'static,
        FV: Fn(&T) -> V + Send + Sync + Clone + 'static,
    {
        let input_id = input.id;
        let key_fn_clone = key_fn.clone();
        let value_fn_clone = value_fn.clone();

        let id = self.graph.create_derived::<(K, V)>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<(K, V)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(K, V)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        // Store the recompute function using BTreeMap-based group_min
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            let (_, output) =
                operators::group_min_init(&input_coll, &key_fn_clone, &value_fn_clone);
            Box::new(output) as Box<dyn AnyCollection>
        }));

        // Compute initial state using BTreeMap-based group_min
        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let (_, output) = operators::group_min_init(&input_coll, &key_fn, &value_fn);
        self.graph.get_mut(id).state = Box::new(output);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(K, V)>());

        Relation::new(id)
    }

    /// Group by key and compute sum of values.
    ///
    /// For each distinct key K, outputs (K, sum(V)) where V are all i64 values
    /// associated with that key, weighted by multiplicity.
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

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::aggregate(
                &input_coll,
                &key_fn_clone,
                &value_fn_clone,
                |k, vals| operators::sum(k, vals),
            )) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::aggregate(&input_coll, &key_fn, &value_fn, |k, vals| {
            operators::sum(k, vals)
        });
        self.graph.get_mut(id).state = Box::new(output);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(K, i64)>());

        Relation::new(id)
    }

    /// Group by key and count tuples.
    ///
    /// For each distinct key K, outputs (K, count) where count is the number
    /// of tuples with that key (weighted by multiplicity).
    pub fn group_count<T, K, FK>(&mut self, input: Relation<T>, key_fn: FK) -> Relation<(K, i64)>
    where
        T: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        FK: Fn(&T) -> K + Send + Sync + Clone + 'static,
    {
        let input_id = input.id;
        let key_fn_clone = key_fn.clone();

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

        // Store the recompute function
        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::aggregate(
                &input_coll,
                &key_fn_clone,
                |_| (),
                |k, vals| operators::count(k, vals),
            )) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::aggregate(
            &input_coll,
            &key_fn,
            |_| (),
            |k, vals| operators::count(k, vals),
        );
        self.graph.get_mut(id).state = Box::new(output);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(K, i64)>());

        Relation::new(id)
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
