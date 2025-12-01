//! Convenience methods for common relation patterns.

use std::hash::Hash;

use super::relation::{Op, Relation};

impl<R> Relation<R> {
    /// Extract the first element of a tuple relation.
    /// Equivalent to `.map(|(a, _)| a)`.
    pub fn fst<A, B>(self) -> Relation<impl Op<A>>
    where
        R: Op<(A, B)>,
    {
        self.map(|(a, _)| a)
    }

    /// Extract the second element of a tuple relation.
    /// Equivalent to `.map(|(_, b)| b)`.
    pub fn snd<A, B>(self) -> Relation<impl Op<B>>
    where
        R: Op<(A, B)>,
    {
        self.map(|(_, b)| b)
    }

    /// Swap the elements of a tuple relation.
    /// Equivalent to `.map(|(a, b)| (b, a))`.
    pub fn swap<A, B>(self) -> Relation<impl Op<(B, A)>>
    where
        R: Op<(A, B)>,
    {
        self.map(|(a, b)| (b, a))
    }

    /// Global maximum - finds the max value across all tuples.
    /// Returns a relation with a single max value.
    pub fn global_max<V>(self) -> Relation<impl Op<V>>
    where
        R: Op<V>,
        V: Clone + Ord,
    {
        self.map(|v| ((), v)).group_max().map(|((), v)| v)
    }

    /// Global minimum - finds the min value across all tuples.
    /// Returns a relation with a single min value.
    pub fn global_min<V>(self) -> Relation<impl Op<V>>
    where
        R: Op<V>,
        V: Clone + Ord,
    {
        self.map(|v| ((), v)).group_min().map(|((), v)| v)
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
        self.join(right).map(|(_k, (v1, v2))| (v1, v2))
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
        self.map(|t| ((), t))
            .join(right.map(|t| ((), t)))
            .map(|((), (t1, t2))| (t1, t2))
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
        self.join(right.map(|k| (k, ()))).map(|(k, (v, ()))| (k, v))
    }

    /// Set difference (self - right).
    /// Implemented via antijoin: treat tuples as (T, ()) pairs.
    pub fn difference<T, RR>(self, right: Relation<RR>) -> Relation<impl Op<T>>
    where
        T: Clone + Eq + Hash,
        R: Op<T>,
        RR: Op<T>,
    {
        self.map(|t: T| (t, ())).antijoin(right).map(|(t, ())| t)
    }
}
