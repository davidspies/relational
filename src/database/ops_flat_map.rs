//! FlatMap operator for Database.

use std::sync::Arc;

use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::Relation;
use crate::Tuple;

use super::Database;

impl Database {
    /// FlatMap: transform each tuple into zero or more tuples.
    pub fn flat_map<T, U, I, F>(&mut self, input: Relation<T>, f: F) -> Relation<U>
    where
        T: Tuple + Send + Sync,
        U: Tuple + Send + Sync,
        I: IntoIterator<Item = U>,
        F: Fn(&T) -> I + Send + Sync + 'static,
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
                Box::new(operators::flat_map_changes(changes, |t| f_inc(t))) as Box<dyn AnyChanges>
            },
        ));

        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            let mut output = Multiset::<U>::new();
            for t in input_coll.iter() {
                for u in f_recompute(t) {
                    output.insert(u);
                }
            }
            Box::new(output) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();

        let mut output = Multiset::<U>::new();
        for t in input_coll.iter() {
            for u in f(t) {
                output.insert(u);
            }
        }
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }
}
