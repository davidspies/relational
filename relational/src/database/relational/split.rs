//! Split - split a relation of pairs into two relations.

use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use contiguous_data::{Diff, Multiset};

use crate::database::commit_id::CommitId;

use super::op::Op;
use super::relation::Relation;

/// Shared state for a split relation.
struct SplitState<A, B, R: Op<(A, B)>> {
    upstream: Relation<R>,
    left_queue: Multiset<A>,
    right_queue: Multiset<B>,
    last_update_commit_id: CommitId,
}

impl<A: Eq + Hash, B: Eq + Hash, R: Op<(A, B)>> SplitState<A, B, R> {
    fn update(&mut self) {
        let current = self.upstream.commit_id.get();
        if current == self.last_update_commit_id {
            return;
        }
        self.last_update_commit_id = current;

        self.upstream.foreach(|(a, b), diff| {
            self.left_queue.update(a, diff);
            self.right_queue.update(b, diff);
        });
    }
}

/// Left side of a split relation.
pub struct SplitLeft<A, B, R: Op<(A, B)>> {
    state: Rc<RefCell<SplitState<A, B, R>>>,
}

impl<A: Eq + Hash, B: Eq + Hash, R: Op<(A, B)>> Op<A> for SplitLeft<A, B, R> {
    fn foreach(&mut self, mut consumer: impl FnMut(A, Diff)) {
        self.state.borrow_mut().update();
        for (a, diff) in self.state.borrow_mut().left_queue.drain() {
            consumer(a, diff);
        }
    }
}

/// Right side of a split relation.
pub struct SplitRight<A, B, R: Op<(A, B)>> {
    state: Rc<RefCell<SplitState<A, B, R>>>,
}

impl<A: Eq + Hash, B: Eq + Hash, R: Op<(A, B)>> Op<B> for SplitRight<A, B, R> {
    fn foreach(&mut self, mut consumer: impl FnMut(B, Diff)) {
        self.state.borrow_mut().update();
        for (b, diff) in self.state.borrow_mut().right_queue.drain() {
            consumer(b, diff);
        }
    }
}

impl<R> Relation<R> {
    /// Split a relation of pairs into two separate relations.
    ///
    /// Unlike `save()`, this doesn't require Clone - it decomposes the tuple.
    pub fn split<A, B>(self) -> (Relation<SplitLeft<A, B, R>>, Relation<SplitRight<A, B, R>>)
    where
        A: Eq + Hash,
        B: Eq + Hash,
        R: Op<(A, B)>,
    {
        let upstream_node_id = self.node_id;
        let commit_id = self.commit_id.clone();
        let graph = self.graph.clone();

        let state = Rc::new(RefCell::new(SplitState {
            upstream: self,
            left_queue: Multiset::new(),
            right_queue: Multiset::new(),
            last_update_commit_id: CommitId::default(),
        }));

        let left = Relation::new(
            SplitLeft {
                state: state.clone(),
            },
            commit_id.clone(),
            graph.clone(),
            "split_left",
            vec![upstream_node_id],
        );

        let right = Relation::new(
            SplitRight { state },
            commit_id,
            graph,
            "split_right",
            vec![upstream_node_id],
        );

        (left, right)
    }
}
