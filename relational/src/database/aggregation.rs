//! Min/max aggregation operators for Database.

use std::cmp::Reverse;

use crate::Tuple;
use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::Relation;

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

        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .expect("type mismatch in group_max recompute")
                .clone();
            let (_, output) =
                operators::group_max_init(&input_coll, &key_fn_clone, &value_fn_clone);
            Box::new(output) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .expect("type mismatch in group_max initial")
            .clone();
        let (_, output) = operators::group_max_init(&input_coll, &key_fn, &value_fn);
        self.graph.get_mut(id).state = Box::new(output);
        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(K, V)>());

        Relation::new(id)
    }

    /// Group by key and compute minimum value.
    ///
    /// Implemented as group_max with Reverse, then unwrapping.
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
        let with_reverse = self.group_max(input, key_fn, move |t| Reverse(value_fn(t)));
        self.map(with_reverse, |(k, Reverse(v))| (k.clone(), v.clone()))
    }
}
