//! Filter operator for Database.

use std::sync::Arc;

use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::Relation;
use crate::Tuple;

use super::Database;

impl Database {
    /// Filter: keep tuples matching predicate.
    pub fn filter<T, F>(&mut self, input: Relation<T>, pred: F) -> Relation<T>
    where
        T: Tuple + Send + Sync,
        F: Fn(&T) -> bool + Send + Sync + 'static,
    {
        let pred = Arc::new(pred);
        let pred_inc = pred.clone();
        let pred_recompute = pred.clone();
        let input_id = input.id;

        let id = self.graph.create_derived::<T>(
            None,
            vec![input.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<T>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<T>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<T>());

        self.incremental_fns[id.index()] = Some(Box::new(
            move |_graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);
                Box::new(operators::filter_changes(changes, |t| pred_inc(t))) as Box<dyn AnyChanges>
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
            Box::new(operators::filter(&input_coll, |t| pred_recompute(t)))
                as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::filter(&input_coll, |t| pred(t));
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }
}
