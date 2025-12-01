//! Convenience methods for common relation patterns.

use std::{cmp::Reverse, hash::Hash};

use super::relation::{Op, Relation};

impl<R> Relation<R> {
    /// Extract the first element of a tuple relation.
    /// Equivalent to `.map(|(a, _)| a)`.
    pub fn fst<A, B>(self) -> Relation<impl Op<A>>
    where
        R: Op<(A, B)>,
        A: Eq + Hash,
    {
        self.map(|(a, _)| a).with_op_type("fst")
    }

    /// Extract the second element of a tuple relation.
    /// Equivalent to `.map(|(_, b)| b)`.
    pub fn snd<A, B>(self) -> Relation<impl Op<B>>
    where
        R: Op<(A, B)>,
        B: Eq + Hash,
    {
        self.map(|(_, b)| b).with_op_type("snd")
    }

    /// Swap the elements of a tuple relation.
    /// Equivalent to `.map(|(a, b)| (b, a))`.
    pub fn swap<A, B>(self) -> Relation<impl Op<(B, A)>>
    where
        R: Op<(A, B)>,
        A: Eq + Hash,
        B: Eq + Hash,
    {
        self.map(|(a, b)| (b, a)).with_op_type("swap")
    }

    /// Global maximum - finds the max value across all tuples.
    /// Returns a relation with a single max value.
    pub fn global_max<V>(self) -> Relation<impl Op<V>>
    where
        R: Op<V>,
        V: Clone + Eq + Hash + Ord,
    {
        self.map_h(|v| ((), v))
            .group_max()
            .with_op_type("global_max")
            .map_h(|((), v)| v)
    }

    /// Global minimum - finds the min value across all tuples.
    /// Returns a relation with a single min value.
    pub fn global_min<V>(self) -> Relation<impl Op<V>>
    where
        R: Op<V>,
        V: Clone + Eq + Hash + Ord,
    {
        self.map_h(|v| ((), v))
            .group_min()
            .with_op_type("global_min")
            .map_h(|((), v)| v)
    }

    /// Join two relations and discard the key.
    /// Both inputs must be `(K, V)` tuples. Output is `(V1, V2)` pairs.
    pub fn join_values<K, V1, V2, RR>(self, right: Relation<RR>) -> Relation<impl Op<(V1, V2)>>
    where
        R: Op<(K, V1)>,
        K: Clone + Eq + Hash,
        V1: Clone + Eq + Hash,
        V2: Clone + Eq + Hash,
        RR: Op<(K, V2)>,
    {
        self.join(right)
            .with_op_type("join_values")
            .map_h(|(_k, (v1, v2))| (v1, v2))
    }

    /// Cartesian product - pairs every tuple from left with every tuple from right.
    /// Output is `(T1, T2)` pairs.
    pub fn cartesian_product<T1, T2, RR>(self, right: Relation<RR>) -> Relation<impl Op<(T1, T2)>>
    where
        R: Op<T1>,
        T1: Clone + Eq + Hash,
        T2: Clone + Eq + Hash,
        RR: Op<T2>,
    {
        self.map_h(|t| ((), t))
            .join(right.map_h(|t| ((), t)))
            .with_op_type("cartesian_product")
            .map_h(|((), (t1, t2))| (t1, t2))
    }

    /// Semijoin - filter left relation to only tuples that have a matching key in right.
    /// Input: left is `(K, V)`, right is any relation of keys `K`.
    /// Output: `(K, V)` tuples from left where key exists in right.
    pub fn semijoin<K, V, RR>(self, right: Relation<RR>) -> Relation<impl Op<(K, V)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash,
        RR: Op<K>,
    {
        self.join(right.map_h(|k| (k, ())))
            .with_op_type("semijoin")
            .map_h(|(k, (v, ()))| (k, v))
    }

    /// Set difference (self - right).
    /// Implemented via antijoin: treat tuples as (T, ()) pairs.
    pub fn difference<T, RR>(self, right: Relation<RR>) -> Relation<impl Op<T>>
    where
        T: Clone + Eq + Hash,
        R: Op<T>,
        RR: Op<T>,
    {
        self.map_h(|t: T| (t, ()))
            .antijoin(right)
            .with_op_type("difference")
            .map_h(|(t, ())| t)
    }

    /// Set intersection (self ∩ right).
    /// Keeps only tuples that appear in both relations.
    pub fn intersection<T, RR>(self, right: Relation<RR>) -> Relation<impl Op<T>>
    where
        T: Clone + Eq + Hash,
        R: Op<T>,
        RR: Op<T>,
    {
        self.map_h(|t: T| (t, ()))
            .semijoin(right)
            .with_op_type("intersection")
            .map_h(|(t, ())| t)
    }

    /// Count tuples by key.
    /// Input must be (K, V) tuples where K is the key.
    /// Output is (K, count) pairs.
    pub fn counts<T>(self) -> Relation<impl Op<(T, i64)>>
    where
        R: Op<T>,
        T: Clone + Eq + Hash,
    {
        self.map_h(|t| (t, 1i64)).group_sum().with_op_type("counts")
    }

    /// Minimum value by key.
    /// Input must be (K, V) tuples where K is the key and V is the value.
    pub fn group_min<K, V>(self) -> Relation<impl Op<(K, V)>>
    where
        R: Op<(K, V)>,
        K: Clone + Eq + Hash,
        V: Clone + Eq + Hash + Ord,
    {
        self.map_h(|(k, v)| (k, Reverse(v)))
            .group_max()
            .with_op_type("group_min")
            .map_h(|(k, Reverse(v))| (k, v))
    }

    /// Filter tuples by predicate.
    pub fn filter<T, F>(self, pred: F) -> Relation<impl Op<T>>
    where
        R: Op<T>,
        F: Fn(&T) -> bool,
        T: Eq + Hash,
    {
        self.flat_map(move |t| if pred(&t) { Some(t) } else { None })
            .with_op_type("filter")
    }
}
