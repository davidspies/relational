//! Map operator for Database.

use std::sync::Arc;

use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::Relation;
use crate::Tuple;

use super::Database;

impl Database {
    /// Map: transform each tuple.
    pub fn map<T, U, F>(&mut self, input: Relation<T>, f: F) -> Relation<U>
    where
        T: Tuple + Send + Sync,
        U: Tuple + Send + Sync,
        F: Fn(&T) -> U + Send + Sync + 'static,
    {
        let f = Arc::new(f);
        let f_inc = f.clone();
        let f_recompute = f.clone();
        let input_id = input.id;

        let id = self.graph.create_derived::<U>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<U>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<U>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<U>());

        self.incremental_fns[id.index()] = Some(Box::new(
            move |_graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);
                Box::new(operators::map_changes(changes, |t| f_inc(t))) as Box<dyn AnyChanges>
            },
        ));

        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .expect("type mismatch in map recompute")
                .clone();
            Box::new(operators::map(&input_coll, |t| f_recompute(t))) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .expect("type mismatch in map initial")
            .clone();
        let output = operators::map(&input_coll, |t| f(t));
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }
}
