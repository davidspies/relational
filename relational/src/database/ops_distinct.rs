//! Distinct operator for Database.

use crate::Tuple;
use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::Relation;

use super::Database;

impl Database {
    /// Distinct: remove duplicate tuples (ensure multiplicity = 1).
    pub fn distinct<T>(&mut self, input: Relation<T>) -> Relation<T>
    where
        T: Tuple + Send + Sync,
    {
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

        // Distinct incremental: compute old state by reversing changes
        self.incremental_fns[id.index()] = Some(Box::new(
            move |graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<T>>>()
                    .expect("type mismatch in distinct incremental changes");

                let new_input = graph
                    .get(input_id)
                    .state
                    .as_any()
                    .downcast_ref::<Multiset<T>>()
                    .expect("type mismatch in distinct incremental state")
                    .clone();

                let mut old_input = new_input.clone();
                for change in changes {
                    old_input.apply_change(change.negate());
                }

                Box::new(operators::distinct_changes(&old_input, &new_input)) as Box<dyn AnyChanges>
            },
        ));

        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let input_coll = graph
                .get(input_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .expect("type mismatch in distinct recompute")
                .clone();
            Box::new(operators::distinct(&input_coll)) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .expect("type mismatch in distinct initial")
            .clone();
        let output = operators::distinct(&input_coll);
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }
}
