//! Set operators: union, distinct, difference.

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
                .cloned()
                .unwrap_or_default();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            Box::new(operators::union(&left_coll, &right_coll)) as Box<dyn AnyCollection>
        }));

        let left_coll = self
            .graph
            .get(left.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();

        let output = operators::union(&left_coll, &right_coll);
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

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
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);

                let new_input = graph
                    .get(input_id)
                    .state
                    .as_any()
                    .downcast_ref::<Multiset<T>>()
                    .cloned()
                    .unwrap_or_default();

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
                .cloned()
                .unwrap_or_default();
            Box::new(operators::distinct(&input_coll)) as Box<dyn AnyCollection>
        }));

        let input_coll = self
            .graph
            .get(input.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let output = operators::distinct(&input_coll);
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }

    /// Difference: tuples in left but not in right.
    pub fn difference<T>(&mut self, left: Relation<T>, right: Relation<T>) -> Relation<T>
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

        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let left_coll = graph
                .get(left_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .cloned()
                .unwrap_or_default();
            let mut output = left_coll.clone();
            for (t, diff) in right_coll.iter_with_multiplicity() {
                output.apply_change(Change::new(t.clone(), crate::change::Diff(-diff.0)));
            }
            output.compact();
            Box::new(operators::distinct(&output)) as Box<dyn AnyCollection>
        }));

        let left_coll = self
            .graph
            .get(left.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .cloned()
            .unwrap_or_default();

        let mut output = left_coll.clone();
        for (t, diff) in right_coll.iter_with_multiplicity() {
            output.apply_change(Change::new(t.clone(), crate::change::Diff(-diff.0)));
        }
        output.compact();
        let output = operators::distinct(&output);

        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }
}
