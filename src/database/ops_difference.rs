//! Difference operator for Database.

use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::Relation;
use crate::Tuple;

use super::Database;

impl Database {
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
                .expect("type mismatch in difference recompute left")
                .clone();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<T>>()
                .expect("type mismatch in difference recompute right")
                .clone();
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
            .expect("type mismatch in difference initial left")
            .clone();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<T>>()
            .expect("type mismatch in difference initial right")
            .clone();

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
