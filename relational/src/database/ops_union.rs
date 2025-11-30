//! Union operator for Database.

use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::Relation;
use crate::Tuple;

use super::Database;

impl Database {
    /// Union: combine two relations.
    pub fn union<T>(&mut self, left: Relation<T>, right: Relation<T>) -> Relation<T>
    where
        T: Tuple + Send + Sync,
    {
        let left_id = left.id;
        let right_id = right.id;

        let id = self.graph.create_derived::<T>(
            None,
            vec![left.id, right.id],
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
                // Note: downcast may fail if input has no changes (empty changes use unit type)
                let left_changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .cloned()
                    .unwrap_or_default();
                let right_changes = input_changes[1]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .cloned()
                    .unwrap_or_default();

                let mut output = left_changes;
                output.extend(right_changes);
                Box::new(output) as Box<dyn AnyChanges>
            },
        ));

        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let left_coll = graph
                .get(left_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .expect("type mismatch in union recompute left")
                .clone();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .expect("type mismatch in union recompute right")
                .clone();
            Box::new(operators::union(&left_coll, &right_coll)) as Box<dyn AnyCollection>
        }));

        let left_coll = self
            .graph
            .get(left.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .expect("type mismatch in union initial left")
            .clone();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .expect("type mismatch in union initial right")
            .clone();

        let output = operators::union(&left_coll, &right_coll);
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }
}
