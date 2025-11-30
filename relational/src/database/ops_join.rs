//! Join operator implementation.

use std::sync::Arc;

use crate::change::Change;
use crate::collection::Multiset;
use crate::dataflow::{AnyChanges, AnyCollection, DataflowGraph};
use crate::operators;
use crate::relation::Relation;
use crate::Tuple;

use super::Database;

impl Database {
    /// Join two relations on a key.
    pub fn join<L, R, K, FL, FR>(
        &mut self,
        left: Relation<L>,
        right: Relation<R>,
        key_left: FL,
        key_right: FR,
    ) -> Relation<(L, R)>
    where
        L: Tuple + Send + Sync,
        R: Tuple + Send + Sync,
        K: Tuple + Send + Sync,
        FL: Fn(&L) -> K + Send + Sync + 'static,
        FR: Fn(&R) -> K + Send + Sync + 'static,
    {
        let key_left = Arc::new(key_left);
        let key_right = Arc::new(key_right);
        let kl_inc = key_left.clone();
        let kr_inc = key_right.clone();
        let kl_recompute = key_left.clone();
        let kr_recompute = key_right.clone();
        let left_id = left.id;
        let right_id = right.id;

        let id = self.graph.create_derived::<(L, R)>(
            None,
            vec![left.id, right.id],
            Box::new(|_, _| {
                (
                    Box::new(Multiset::<(L, R)>::new()) as Box<dyn AnyCollection>,
                    Box::new(Vec::<Change<(L, R)>>::new()) as Box<dyn AnyChanges>,
                )
            }),
            None,
        );
        self.ensure_recompute_fns_len(id);

        self.apply_fns[id.index()] = Some(Self::make_apply_fn::<(L, R)>());

        // Join incremental: left_changes × old_right + new_left × right_changes
        self.incremental_fns[id.index()] = Some(Box::new(
            move |graph: &DataflowGraph, input_changes: &[&dyn AnyChanges]| {
                let left_changes = input_changes[0]
                    .as_any()
                    .downcast_ref::<Vec<Change<L>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);
                let right_changes = input_changes[1]
                    .as_any()
                    .downcast_ref::<Vec<Change<R>>>()
                    .map(|c| c.as_slice())
                    .unwrap_or(&[]);

                let new_left_state = graph
                    .get(left_id)
                    .state
                    .as_any()
                    .downcast_ref::<Multiset<L>>()
                    .expect("type mismatch in join incremental left")
                    .clone();
                let new_right_state = graph
                    .get(right_id)
                    .state
                    .as_any()
                    .downcast_ref::<Multiset<R>>()
                    .expect("type mismatch in join incremental right")
                    .clone();

                // Compute old_right by reversing right_changes
                let mut old_right_state = new_right_state;
                for change in right_changes {
                    old_right_state.apply_change(change.negate());
                }

                let mut output = Vec::new();

                if !left_changes.is_empty() {
                    output.extend(operators::join_changes_left(
                        left_changes,
                        &old_right_state,
                        |l| kl_inc(l),
                        |r| kr_inc(r),
                    ));
                }

                if !right_changes.is_empty() {
                    output.extend(operators::join_changes_right(
                        &new_left_state,
                        right_changes,
                        |l| kl_inc(l),
                        |r| kr_inc(r),
                    ));
                }

                Box::new(output) as Box<dyn AnyChanges>
            },
        ));

        self.recompute_fns[id.index()] = Some(Box::new(move |graph: &DataflowGraph| {
            let left_coll = graph
                .get(left_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<L>>()
                .expect("type mismatch in join recompute left")
                .clone();
            let right_coll = graph
                .get(right_id)
                .state
                .as_any()
                .downcast_ref::<Multiset<R>>()
                .expect("type mismatch in join recompute right")
                .clone();
            let output = operators::join(
                &left_coll,
                &right_coll,
                |l| kl_recompute(l),
                |r| kr_recompute(r),
            );
            Box::new(output) as Box<dyn AnyCollection>
        }));

        let left_coll = self
            .graph
            .get(left.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<L>>()
            .expect("type mismatch in join initial left")
            .clone();
        let right_coll = self
            .graph
            .get(right.id)
            .state
            .as_any()
            .downcast_ref::<Multiset<R>>()
            .expect("type mismatch in join initial right")
            .clone();

        let output = operators::join(&left_coll, &right_coll, |l| key_left(l), |r| key_right(r));
        self.graph.get_mut(id).state = Box::new(output);

        Relation::new(id)
    }
}
